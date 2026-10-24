import { createServer, type Server, type Socket, connect } from "node:net";
import { createSocket, type Socket as UdpSocket } from "node:dgram";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { $, browser, expect } from "@wdio/globals";
import { app, bot, botInBackground, netsim, testenv, type BackgroundBot } from "../../helpers/index.ts";
import { readWavPcm, waitForText, writeWavPcm } from "../../helpers/wizard-util.ts";

// AC-1.21 (reconnect), AC-1.22 (UDP blocked -> TCP), AC-1.23 (10 % loss -> bitrate adapts).
// The app talks to a counting relay (64802) that forwards to netsim (64800) -> test server v15.
// The relay exists only because the UI does not expose the encoder bitrate: we measure the size
// of the app's UDP voice packets instead.

const RELAY_PORT = 64802;
const ME = "NetTester";
const LINK = `mumble://${ME}@127.0.0.1:${String(RELAY_PORT)}/Raid`;

// ---- counting relay (TCP + UDP) ----
let tcpServer: Server | undefined;
let udpFront: UdpSocket | undefined;
const udpUpstream = new Map<string, UdpSocket>();
let udpSizes: number[] = [];
const tcpSockets = new Set<Socket>();

function startRelay(): Promise<void> {
  tcpServer = createServer((client) => {
    const up = connect({ port: netsim.listenPort, host: "127.0.0.1" });
    tcpSockets.add(client);
    tcpSockets.add(up);
    const closeBoth = () => {
      client.destroy();
      up.destroy();
    };
    client.on("error", closeBoth).on("close", closeBoth);
    up.on("error", closeBoth).on("close", closeBoth);
    client.pipe(up);
    up.pipe(client);
  });
  udpFront = createSocket("udp4");
  udpFront.on("message", (msg, rinfo) => {
    udpSizes.push(msg.length);
    const key = `${rinfo.address}:${String(rinfo.port)}`;
    let upstream = udpUpstream.get(key);
    if (!upstream) {
      upstream = createSocket("udp4");
      upstream.on("message", (m) => {
        udpFront?.send(m, rinfo.port, rinfo.address);
      });
      upstream.on("error", () => undefined);
      udpUpstream.set(key, upstream);
    }
    upstream.send(msg, netsim.listenPort, "127.0.0.1");
  });
  udpFront.bind(RELAY_PORT, "127.0.0.1");
  return new Promise((resolve) => {
    tcpServer?.listen(RELAY_PORT, "127.0.0.1", () => {
      resolve();
    });
  });
}

function stopRelay(): void {
  for (const s of tcpSockets) s.destroy();
  tcpServer?.close();
  udpFront?.close();
  for (const s of udpUpstream.values()) s.close();
}

/** Estimated encoder bitrate (kbit/s) from the app's UDP voice packets (>= 40 bytes). */
function relayBitrate(secs: number): { kbps: number; avgBytes: number; voicePackets: number } {
  const voice = udpSizes.filter((n) => n >= 40);
  const avg = voice.length ? voice.reduce((a, b) => a + b, 0) / voice.length : 0;
  return { kbps: (voice.length * avg * 8) / secs / 1000, avgBytes: avg, voicePackets: voice.length };
}

// ---- audio helpers ----
const dir = mkdtempSync(join(tmpdir(), "mumcord-net-"));
const inWav = join(dir, "in.wav");
const outWav = join(dir, "out.wav");
const botWav = join(dir, "bot.wav");

/** Continuous voiced signal (no pauses), so any silent stretch in a recording is a dropout. */
function synthContinuous(path: string, seconds: number): void {
  const rate = 48_000;
  const pcm = new Int16Array(rate * seconds);
  let phase = 0;
  for (let i = 0; i < pcm.length; i++) {
    const t = i / rate;
    const f0 = 125 + 15 * Math.sin(2 * Math.PI * 0.7 * t);
    phase += (2 * Math.PI * f0) / rate;
    let s = 0;
    for (let h = 1; h <= 20; h++) s += Math.sin(h * phase) / h ** 1.3;
    const env = 0.75 + 0.25 * Math.sin(2 * Math.PI * 3.1 * t);
    pcm[i] = Math.round(s * env * 5500);
  }
  writeWavPcm(path, pcm);
}

interface Gaps {
  seconds: number;
  activeDb: number;
  longestSilentMs: number;
  silentRuns200ms: number;
}

/** 20 ms windows; "silent" = below -55 dBFS. Reports the longest silent run. */
function analyse(pcm: Int16Array): Gaps {
  const n = 960;
  let run = 0;
  let longest = 0;
  let runs200 = 0;
  let sum = 0;
  let cnt = 0;
  for (let i = 0; i + n <= pcm.length; i += n) {
    let e = 0;
    for (let j = i; j < i + n; j++) e += ((pcm[j] ?? 0) / 32768) ** 2;
    const db = 10 * Math.log10(Math.max(e / n, 1e-12));
    if (db < -55) {
      run++;
      if (run > longest) longest = run;
      if (run === 10) runs200++;
    } else {
      run = 0;
      sum += db;
      cnt++;
    }
  }
  return {
    seconds: pcm.length / 48_000,
    activeDb: cnt ? sum / cnt : -120,
    longestSilentMs: longest * 20,
    silentRuns200ms: runs200,
  };
}

function appOutLen(): number {
  return readWavPcm(outWav).length;
}

// ---- bot script helper ----
function scriptBot(name: string): BackgroundBot & { stdout: () => string } {
  const b = botInBackground(["script"], { name, timeout: 30 });
  let out = "";
  b.process.stdout?.on("data", (d: Buffer) => (out += d.toString()));
  return Object.assign(b, { stdout: () => out });
}

async function waitOk(b: { stdout: () => string }, line: string, timeoutMs: number): Promise<void> {
  const t0 = Date.now();
  while (!b.stdout().includes(`ok ${line}`)) {
    if (Date.now() - t0 > timeoutMs) throw new Error(`bot never finished "${line}"`);
    await new Promise((r) => setTimeout(r, 200));
  }
}

/** Bot "Listener" joins Raid and records the app's voice for `secs`. Returns the analysed WAV. */
async function botHearsApp(secs: number, tag: string): Promise<Gaps> {
  const file = join(dir, `heard-${tag}.wav`);
  const l = scriptBot("Listener");
  l.send("join Raid");
  l.send(`record ${file} --seconds ${String(secs)} --from ${ME}`);
  l.process.stdin?.end();
  await l.done;
  return analyse(readWavPcm(file));
}

async function barText(): Promise<string> {
  const bar = $(".servers-status");
  return (await bar.isExisting()) ? await bar.getText() : "";
}

async function waitBar(re: RegExp, timeout: number, poll = 100): Promise<string> {
  let seen = "";
  await browser.waitUntil(
    async () => {
      seen = await barText();
      return re.test(seen);
    },
    { timeout, interval: poll, timeoutMsg: `bar never matched ${String(re)}; last: "${seen}"` },
  );
  return seen;
}

async function waitNoBar(timeout: number): Promise<void> {
  await browser.waitUntil(async () => (await barText()) === "", {
    timeout,
    timeoutMsg: `bar still shown: "${await barText()}"`,
  });
}

async function followButtonText(): Promise<string> {
  return await $("button.follow-toggle").getText();
}

describe("network resilience (AC-1.21, AC-1.22, AC-1.23)", () => {
  let talker: (BackgroundBot & { stdout: () => string }) | undefined;

  before(async () => {
    await testenv.up("v15");
    await netsim.start("v15");
    await startRelay();
    synthContinuous(inWav, 600);
    synthContinuous(botWav, 600);
    await app.restart({ audio: `file:${inWav}:${outWav}` });
  });

  after(async () => {
    talker?.stop();
    stopRelay();
    await netsim.ctl("reset").catch(() => undefined);
    netsim.stop();
  });

  it("setup: wizard, connect through relay, join Raid, follow Lobby", async () => {
    await $("#root").waitForDisplayed();
    await $("input[type=text]").setValue(ME);
    await $("button=Next").click();
    await waitForText("Step 2 of 3");
    await $("button=Next").click();
    await waitForText("Step 3 of 3");
    await $("button=Finish").click();
    const address = $("input[placeholder='mumble.example.org']");
    await address.waitForDisplayed();
    await address.setValue(LINK);
    await $("button=Add and connect").click();
    await $("button=Trust and connect").waitForDisplayed({ timeout: 20_000 });
    await $("button=Trust and connect").click();
    await bot(["expect", "user", ME, "--in", "Raid"], { name: "Watcher", timeout: 30 });
    // Select Lobby (single click) and follow it.
    await $("//*[@role='treeitem'][.//*[normalize-space()='Lobby']]").click();
    await $("button.follow-toggle").waitForDisplayed();
    if (!(await followButtonText()).includes("Following")) {
      await $("button.follow-toggle").click();
    }
    await browser.waitUntil(async () => (await followButtonText()).includes("Following"));
    await bot(["expect", "user", ME, "--in", "Raid", "--listening", "Lobby"], {
      name: "Watcher",
      timeout: 15,
    });
  });

  it("AC-1.21a: server container restart -> Reconnecting bar -> back in Raid, Lobby followed", async () => {
    const restartDone = testenv.restart("v15");
    const bar = await waitBar(/Reconnecting/, 60_000, 50);
    console.log(`AC-1.21a bar text: "${bar}"`);
    await restartDone;
    await waitNoBar(90_000);
    await bot(["expect", "user", ME, "--in", "Raid", "--listening", "Lobby"], {
      name: "Watcher",
      timeout: 30,
    });
    expect(await followButtonText()).toContain("Following");
  });

  it("AC-1.21b: killing the TCP connection -> Reconnecting bar -> back in Raid, Lobby followed", async () => {
    await netsim.ctl("kill-tcp");
    const bar = await waitBar(/Reconnecting/, 30_000, 50);
    console.log(`AC-1.21b bar text: "${bar}"`);
    await waitNoBar(90_000);
    await bot(["expect", "user", ME, "--in", "Raid", "--listening", "Lobby"], {
      name: "Watcher",
      timeout: 30,
    });
  });

  it("baseline: voice both ways over UDP", async () => {
    talker = scriptBot("Talker");
    talker.send("join Raid");
    talker.send(`play ${botWav} --loop --seconds 590`);
    await waitOk(talker, "join Raid", 20_000);
    await browser.pause(3000);
    udpSizes = [];
    const from = appOutLen();
    const heard = await botHearsApp(12, "base");
    const rate = relayBitrate(12);
    const appHeard = analyse(readWavPcm(outWav).slice(from));
    console.log(`BASELINE bot-hears-app ${JSON.stringify(heard)}`);
    console.log(`BASELINE app-hears-bot ${JSON.stringify(appHeard)}`);
    console.log(`BASELINE app uplink ${JSON.stringify(rate)}`);
    expect(heard.activeDb).toBeGreaterThan(-45);
    expect(appHeard.activeDb).toBeGreaterThan(-45);
    expect(await barText()).toBe("");
  });

  it("AC-1.22: UDP blocked -> bar 'Connected via TCP', voice works both ways", async () => {
    await netsim.ctl("block-udp", "on");
    const t0 = Date.now();
    const bar = await waitBar(/Connected via TCP/, 60_000, 200);
    console.log(`AC-1.22 bar after ${String(Date.now() - t0)} ms: "${bar}"`);
    await browser.pause(3000);
    const from = appOutLen();
    const heard = await botHearsApp(12, "tcp");
    const appHeard = analyse(readWavPcm(outWav).slice(from));
    console.log(`TCP bot-hears-app ${JSON.stringify(heard)}`);
    console.log(`TCP app-hears-bot ${JSON.stringify(appHeard)}`);
    expect(heard.activeDb).toBeGreaterThan(-45);
    expect(appHeard.activeDb).toBeGreaterThan(-45);
    await netsim.ctl("block-udp", "off");
    // Back to UDP: the bar should clear again (reported, not asserted hard).
    const t1 = Date.now();
    try {
      await waitNoBar(60_000);
      console.log(`AC-1.22 bar cleared ${String(Date.now() - t1)} ms after unblocking`);
    } catch (e) {
      console.log(`AC-1.22 bar did NOT clear after unblocking UDP: ${String(e)}`);
    }
  });

  it("AC-1.23: 10 % loss -> audio continues, bitrate falls; removing loss restores it", async () => {
    type Sample = { t: number; kbps: number; transport: string; loss: string };
    const samples: Sample[] = [];
    const t00 = Date.now();
    const readInfo = async (): Promise<Sample | undefined> => {
      try {
        const rows = await $$(".connection-info__row").getElements();
        const m: Partial<Record<string, string>> = {};
        for (const r of rows) {
          const k = await r.$("dt").getText();
          const v = await r.$("dd").getText();
          if (k) m[k.trim()] = v.trim();
        }
        const kbps = Number.parseFloat(m["Send bitrate"] ?? "");
        if (Number.isNaN(kbps)) return undefined;
        return { t: (Date.now() - t00) / 1000, kbps, transport: m.Transport ?? "?", loss: m["Packet loss"] ?? "?" };
      } catch {
        return undefined;
      }
    };
    await $("button[aria-label='Connection info']").click();
    await $(".connection-info__panel").waitForDisplayed({ timeout: 5000 });
    const sampler_ = { running: true };
    const sampler = (async () => {
      while (sampler_.running) {
        const x = await readInfo();
        if (x) samples.push(x);
        await new Promise((r) => setTimeout(r, 1000));
      }
    })();
    const phaseMean = (a: number, b: number): number => {
      const v = samples.filter((x) => x.t >= a && x.t < b).map((x) => x.kbps);
      return v.length ? v.reduce((p, c) => p + c, 0) / v.length : Number.NaN;
    };
    const results: Record<string, unknown> = {};
    const phases: { name: string; start: number; end: number }[] = [];
    let allPkts = 0;
    const heardAll: Gaps[] = [];
    const run = async (name: string, secs: number): Promise<void> => {
      udpSizes = [];
      const from = appOutLen();
      const start = (Date.now() - t00) / 1000;
      const heard = await botHearsApp(secs, name);
      const end = (Date.now() - t00) / 1000;
      phases.push({ name, start, end });
      allPkts = udpSizes.length;
      heardAll.push(heard);
      results[name] = {
        heard,
        app: analyse(readWavPcm(outWav).slice(from)),
        relayAllPkts: allPkts,
        relayPktsPerSec: Math.round(allPkts / secs),
        relayVoiceKbps: relayBitrate(secs).kbps,
      };
    };
    await run("pre", 12);
    const baseEnd = (Date.now() - t00) / 1000;
    await netsim.ctl("loss", "both", "10");
    const lossAt = (Date.now() - t00) / 1000;
    for (let i = 1; i <= 4; i++) await run(`loss${String(i)}`, 20);
    const lossEnd = (Date.now() - t00) / 1000;
    await netsim.ctl("loss", "both", "0");
    const clearAt = (Date.now() - t00) / 1000;
    for (let i = 1; i <= 3; i++) await run(`clear${String(i)}`, 20);
    sampler_.running = false;
    await sampler;
    const base = phaseMean(2, baseEnd);
    const lossTail = phaseMean(lossEnd - 30, lossEnd);
    const lossMin = Math.min(...samples.filter((x) => x.t >= lossAt && x.t < lossEnd).map((x) => x.kbps));
    const recovered = samples.find((x) => x.t >= clearAt && x.kbps >= 0.9 * base);
    const lossTransports = [...new Set(samples.filter((x) => x.t >= lossAt && x.t < lossEnd).map((x) => x.transport))];
    const lossLossVals = [...new Set(samples.filter((x) => x.t >= lossAt && x.t < lossEnd).map((x) => x.loss))];
    const longest = Math.max(...heardAll.map((h) => h.longestSilentMs));
    console.log(`AC-1.23 RESULTS ${JSON.stringify(results, null, 1)}`);
    console.log(`AC-1.23 SAMPLES ${JSON.stringify(samples.map((x) => [Math.round(x.t), x.kbps, x.transport, x.loss]))}`);
    console.log(
      `AC-1.23 SUMMARY baseline=${base.toFixed(1)} lossTail=${lossTail.toFixed(1)} lossMin=${String(lossMin)} recoveredAt=${recovered ? `${(recovered.t - clearAt).toFixed(0)}s` : "never"} (${String(recovered?.kbps)}) transportDuringLoss=${lossTransports.join(",")} reportedLoss=${lossLossVals.join(",")} longestBotGapMs=${String(longest)} phases=${JSON.stringify(phases)}`,
    );
    console.log(`AC-1.23 bar at end: "${await barText()}"`);
    expect(lossTransports).toEqual(["UDP"]);
    expect(longest).toBeLessThan(500);
    expect(lossTail).toBeLessThan(base * 0.95);
    expect(recovered).toBeDefined();
  });
});
