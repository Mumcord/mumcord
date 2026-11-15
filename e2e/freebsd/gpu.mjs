// Optional real-desktop endurance checks for the native FreeBSD runner. Nothing is
// captured from the user's desktop except statistics for the synthetic test pattern.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import process from "node:process";
import { setTimeout as delay } from "node:timers/promises";

export function processTable(text) {
  return text
    .trim()
    .split("\n")
    .slice(1)
    .map((line) => {
      const [pid, ppid, rss, ...command] = line.trim().split(/\s+/);
      return {
        pid: Number(pid),
        ppid: Number(ppid),
        rssKiB: Number(rss),
        command: command.join(" "),
      };
    });
}

export function descendants(rows, rootPid) {
  const owned = new Set([rootPid]);
  for (;;) {
    const before = owned.size;
    for (const row of rows) if (owned.has(row.ppid)) owned.add(row.pid);
    if (owned.size === before) return rows.filter((row) => owned.has(row.pid));
  }
}

export function gpuFaults(text) {
  return text
    .split("\n")
    .filter((line) =>
      /GPU HANG|preemption time.?out|GPU.{0,40}(reset|hang|fault)|(?:i915|drm).{0,100}(hang|reset|fault|timed? out|\*ERROR\*)/i.test(
        line,
      ),
    );
}

export function assertHardwareRenderer(glx) {
  assert.match(glx, /Accelerated: yes/);
  assert.match(glx, /OpenGL renderer string:.*Intel/i);
  assert.doesNotMatch(glx, /llvmpipe|softpipe|swrast/i);
}

function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
}

export function assertStableMemory(samples, pid, startedSeconds) {
  const points = samples.flatMap((sample) => {
    const row = sample.processes.find((row) => row.pid === pid);
    return row ? [{ elapsed: sample.elapsedSeconds - startedSeconds, rss: row.rssKiB }] : [];
  });
  const warm = points.filter((point) => point.elapsed >= 60 && point.elapsed <= 120);
  const last = points.at(-1)?.elapsed ?? 0;
  const end = points.filter((point) => point.elapsed >= last - 60);
  assert(
    warm.length >= 3 && end.length >= 3,
    "not enough memory samples for the receiving WebKit process",
  );
  const warmMiB = median(warm.map((point) => point.rss)) / 1024;
  const endMiB = median(end.map((point) => point.rss)) / 1024;
  assert(
    endMiB - warmMiB <= 256,
    `receiving WebKit memory kept growing: ${warmMiB.toFixed(1)} -> ${endMiB.toFixed(1)} MiB`,
  );
  return { warmMiB, endMiB, peakMiB: Math.max(...points.map((point) => point.rss)) / 1024 };
}

function ps() {
  return processTable(
    execFileSync("ps", ["-ax", "-o", "pid,ppid,rss,comm"], { encoding: "utf8", timeout: 5000 }),
  );
}

function kernel() {
  return execFileSync("dmesg", [], { encoding: "utf8", timeout: 5000 });
}

function oomLines(text) {
  return text
    .split("\n")
    .filter((line) => /out of swap space|was killed: failed to reclaim memory/.test(line));
}

function nativeDeviceEvidence(driverPid) {
  const rows = descendants(ps(), driverPid).filter((row) => /WebKitWebProcess/.test(row.command));
  const evidence = [];
  for (const row of rows) {
    const fds = execFileSync("procstat", ["-f", String(row.pid)], {
      encoding: "utf8",
      timeout: 5000,
    });
    const devices = [...new Set(fds.match(/\/dev\/(?:drm|dri)\/[^\s]+/g) ?? [])];
    if (devices.length === 0) continue;
    const maps = execFileSync("procstat", ["-v", String(row.pid)], {
      encoding: "utf8",
      timeout: 5000,
    });
    const modules = [
      ...new Set(
        maps
          .split("\n")
          .filter((line) => /gallium|iris_dri|EGL_mesa|GLX_mesa/.test(line))
          .map((line) => line.trim().split(/\s+/).at(-1).split("/").at(-1)),
      ),
    ];
    assert(modules.length > 0, "WebKit opened a GPU device without identifiable Mesa modules");
    evidence.push({ pid: row.pid, devices, modules });
  }
  assert(evidence.length > 0, "receiving WebKit process has no native GPU device");
  return evidence;
}

export function startGpuChecks(root, artifacts) {
  // An early failure must not leave an older run's success or memory trace behind.
  for (const name of [
    "gpu-result.json",
    "gpu-soak.json",
    "gpu-devices-before.json",
    "memory.jsonl",
  ]) {
    rmSync(join(artifacts, name), { force: true });
  }
  assert(
    process.env.DISPLAY && process.env.XAUTHORITY,
    "--gpu needs the real X11 DISPLAY and XAUTHORITY",
  );
  assert(
    !/^(1|true)$/i.test(process.env.LIBGL_ALWAYS_SOFTWARE ?? ""),
    "software rendering is forced",
  );
  assert(
    !/^(1|true)$/i.test(process.env.WEBKIT_DISABLE_COMPOSITING_MODE ?? ""),
    "compositing is disabled",
  );
  const glx = execFileSync("glxinfo", ["-B"], { encoding: "utf8", timeout: 15000 });
  assertHardwareRenderer(glx);
  writeFileSync(join(artifacts, "glxinfo.txt"), glx);
  const baseline = kernel();
  assert.deepEqual(gpuFaults(baseline), [], "GPU faults already present before the test");
  const previousOom = new Set(oomLines(baseline));
  const pixelProbe = join(root, "presented-pattern");
  const flags = execFileSync("pkg-config", ["--cflags", "--libs", "x11"], { encoding: "utf8" })
    .trim()
    .split(/\s+/);
  execFileSync(
    "cc",
    [
      "-Wall",
      "-Wextra",
      "-Werror",
      fileURLToPath(new URL("./presented-pattern.c", import.meta.url)),
      "-o",
      pixelProbe,
      ...flags,
    ],
    { timeout: 30000 },
  );
  const started = performance.now();
  const samples = [];
  let failure = null;
  let escalation;
  let lastRows = [];
  const terminateApps = (signal) => {
    for (const row of descendants(lastRows, process.pid).filter((row) =>
      /(^|\/)mumcord$/.test(row.command),
    )) {
      try {
        process.kill(row.pid, signal);
      } catch (error) {
        if (error.code !== "ESRCH")
          console.error(`Could not stop test client ${row.pid}: ${error.message}`);
      }
    }
  };
  const monitor = setInterval(() => {
    try {
      lastRows = ps();
      const processes = descendants(lastRows, process.pid);
      const totalRssKiB = processes.reduce((sum, row) => sum + row.rssKiB, 0);
      const elapsedSeconds = (performance.now() - started) / 1000;
      const sample = { elapsedSeconds, totalRssKiB, processes };
      samples.push(sample);
      writeFileSync(join(artifacts, "memory.jsonl"), JSON.stringify(sample) + "\n", {
        flag: "a",
        mode: 0o600,
      });
      if (totalRssKiB > 3 * 1024 * 1024 || elapsedSeconds > 900) {
        throw new Error(
          `safety limit: ${totalRssKiB} KiB summed test RSS after ${elapsedSeconds.toFixed(1)} seconds`,
        );
      }
    } catch (error) {
      if (failure !== null) return;
      failure = error.message;
      console.error(`GPU CHECK ABORTED: ${failure}; stopping only this run's clients`);
      terminateApps("SIGTERM");
      escalation = setTimeout(() => {
        try {
          lastRows = ps();
        } catch (error) {
          console.error(`Could not refresh test process list: ${error.message}`);
        }
        terminateApps("SIGKILL");
      }, 2000);
    }
  }, 1000);
  return {
    samples,
    elapsedSeconds: () => (performance.now() - started) / 1000,
    pixelProbe,
    check() {
      assert.equal(failure, null, failure ?? "memory guard");
      const text = kernel();
      assert.deepEqual(gpuFaults(text), [], "kernel GPU fault during playback");
      assert.deepEqual(
        oomLines(text).filter((line) => !previousOom.has(line)),
        [],
        "new kernel OOM kill",
      );
    },
    stop() {
      clearInterval(monitor);
      clearTimeout(escalation);
    },
  };
}

export async function gpuSoak({ a, b, serverId, invoke, driverPid, checks, artifacts }) {
  const before = nativeDeviceEvidence(driverPid);
  writeFileSync(join(artifacts, "gpu-devices-before.json"), JSON.stringify(before, null, 2));
  // Only the private source gets a pattern; the real GPU viewer runs the normal UI.
  await a.execute(() => {
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = 128;
    Object.assign(canvas.style, {
      position: "fixed",
      top: "70px",
      left: "12px",
      zIndex: "2147483647",
      pointerEvents: "none",
    });
    document.body.append(canvas);
    const context = canvas.getContext("2d");
    let frame = 0;
    const draw = () => {
      context.fillStyle = `rgb(12, ${Math.round(128 + 60 * Math.sin(frame++ / 30))}, 240)`;
      context.fillRect(0, 0, 128, 128);
      setTimeout(draw, 33);
    };
    draw();
  });
  const started = performance.now();
  const memoryStarted = checks.elapsedSeconds();
  const samples = [];
  console.log(
    "GPU SOAK START: 600 seconds, real hardware-rendered receiver, voice joined, microphones muted",
  );
  while (performance.now() - started < 600000) {
    await delay(Math.min(10000, 600000 - (performance.now() - started)));
    checks.check();
    for (const browser of [a, b]) {
      assert.notEqual(await invoke(browser, "voice_server_get"), null, "voice disconnected");
      assert.equal((await invoke(browser, "connections_list")).length, 1);
    }
    const rect = await b.execute(() => ({
      x: window.screenX,
      y: window.screenY,
      width: window.outerWidth,
      height: window.outerHeight,
      visible: document.visibilityState === "visible",
    }));
    assert(rect.visible, "GPU viewer is not visible");
    const presented = JSON.parse(
      execFileSync(checks.pixelProbe, [rect.x, rect.y, rect.width, rect.height].map(String), {
        encoding: "utf8",
        timeout: 5000,
      }),
    );
    assert(
      presented.matched >= 25,
      "synthetic source pattern missing from the physical viewer window",
    );
    const quality = await invoke(a, "screen_share_quality", { serverId });
    assert(
      quality?.fps > 0 &&
        quality.viewers.some((viewer) => viewer.fps > 0 && viewer.path === "direct"),
      "capture or direct playback stopped",
    );
    const sample = {
      elapsedSeconds: (performance.now() - started) / 1000,
      width: quality.width,
      height: quality.height,
      encoderFps: quality.fps,
      encoder: quality.encoder,
      presented,
    };
    samples.push(sample);
    const recent = samples.slice(-6);
    if (recent.length === 6)
      assert(
        Math.max(...recent.map((s) => s.presented.pixel[1])) -
          Math.min(...recent.map((s) => s.presented.pixel[1])) >
          20,
        "physical viewer picture stopped changing",
      );
    writeFileSync(join(artifacts, "gpu-soak.json"), JSON.stringify(samples, null, 2));
    console.log(
      `GPU ${sample.elapsedSeconds.toFixed(1)}s: ${quality.width}x${quality.height}, encoder ${quality.fps} fps, visible pattern green=${presented.pixel[1]}`,
    );
  }
  checks.check();
  const after = nativeDeviceEvidence(driverPid);
  assert.deepEqual(
    after.map((entry) => entry.pid),
    before.map((entry) => entry.pid),
    "GPU web process restarted",
  );
  const memory = assertStableMemory(checks.samples, before[0].pid, memoryStarted);
  writeFileSync(
    join(artifacts, "gpu-result.json"),
    JSON.stringify(
      { seconds: (performance.now() - started) / 1000, before, after, memory },
      null,
      2,
    ),
  );
  console.log(
    `PASS ten-minute GPU session: no GPU fault, OOM, lost connection or frozen picture; receiving WebKit ${memory.warmMiB.toFixed(1)} -> ${memory.endMiB.toFixed(1)} MiB median RSS`,
  );
}
