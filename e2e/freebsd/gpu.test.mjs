import assert from "node:assert/strict";
import { test } from "node:test";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import process from "node:process";
import {
  assertHardwareRenderer,
  assertStableMemory,
  assertMemoryBudget,
  memoryHeadroomKiB,
  descendants,
  gpuFaults,
  processTable,
  startGpuChecks,
} from "./gpu.mjs";

test("failed GPU startup removes stale success and memory artifacts, not unrelated files", () => {
  const root = mkdtempSync(join(tmpdir(), "mumcord-gpu-test-"));
  const display = process.env.DISPLAY;
  try {
    for (const name of ["gpu-result.json", "memory.jsonl", "keep.log"])
      writeFileSync(join(root, name), "old");
    process.env.DISPLAY = "";
    assert.throws(() => startGpuChecks(root, root), /needs the real X11/);
    assert.equal(existsSync(join(root, "gpu-result.json")), false);
    assert.equal(existsSync(join(root, "memory.jsonl")), false);
    assert.equal(existsSync(join(root, "keep.log")), true);
  } finally {
    if (display === undefined) delete process.env.DISPLAY;
    else process.env.DISPLAY = display;
    rmSync(root, { recursive: true, force: true });
  }
});

test("FreeBSD headroom uses free plus inactive pages and rejects malformed counters", () => {
  assert.equal(memoryHeadroomKiB("4096\n1000000\n500000\n"), 6000000);
  for (const counters of ["4096\n1", "4096\n-1\n0", "0\n1\n2", "4096\nNaN\n0"]) {
    assert.throws(() => memoryHeadroomKiB(counters), /invalid FreeBSD memory/);
  }
});

test("memory guard retains the 3 GiB RSS budget and rejects low host headroom and runaway tests", () => {
  const gib = 1024 * 1024;
  assert.doesNotThrow(() => assertMemoryBudget(2.9 * gib, 600, 2 * gib));
  assert.throws(() => assertMemoryBudget(3.1 * gib, 600, 2 * gib), /RSS exceeds/);
  assert.throws(() => assertMemoryBudget(2 * gib, 600, 0.9 * gib), /host has less/);
  assert.throws(() => assertMemoryBudget(2 * gib, 901, 2 * gib), /fifteen minutes/);
});

test("FreeBSD process columns preserve parents and RSS; ownership is transitive, not global", () => {
  const rows = processTable(
    " PID PPID RSS COMMAND\n 12 11 300 WebKitWebProcess\n 11 10 200 mumcord\n 99 1 999 mumcord\n 10 1 100 node\n",
  );
  assert.deepEqual(
    descendants(rows, 10).map((row) => row.pid),
    [12, 11, 10],
  );
  assert.equal(
    descendants(rows, 10).reduce((sum, row) => sum + row.rssKiB, 0),
    600,
  );
  assert.deepEqual(descendants(rows, 123), []);
});

test("GPU faults do not confuse earlier memory exhaustion or USB timeouts with a GPU hang", () => {
  assert.deepEqual(gpuFaults("swap_pager: out of swap space\nng_ubt: HCI command timed out"), []);
  for (const line of [
    "GPU HANG: ecode",
    "rcs0 preemption time out",
    "drmn0: [drm] Resetting rcs0",
    "i915: render timed out",
  ]) {
    assert.deepEqual(gpuFaults(line), [line]);
  }
});

test("hardware proof rejects software rendering even if a renderer name contains Intel", () => {
  assertHardwareRenderer("Accelerated: yes\nOpenGL renderer string: Mesa Intel(R) UHD Graphics");
  assert.throws(() =>
    assertHardwareRenderer("Accelerated: no\nOpenGL renderer string: Intel llvmpipe"),
  );
  assert.throws(() =>
    assertHardwareRenderer("Accelerated: yes\nOpenGL renderer string: Intel softpipe"),
  );
});

function memorySamples(grow) {
  return Array.from({ length: 601 }, (_, elapsedSeconds) => ({
    elapsedSeconds,
    processes: [
      { pid: 7, rssKiB: (400 + (grow ? elapsedSeconds : elapsedSeconds % 20)) * 1024 },
      { pid: 99, rssKiB: 9000 * 1024 },
    ],
  }));
}

test("steady receiving memory tolerates collection sawtooth and ignores unrelated processes", () => {
  const result = assertStableMemory(memorySamples(false), 7, 0);
  assert.equal(result.peakMiB, 419);
  assert(Math.abs(result.endMiB - result.warmMiB) < 20);
});

test("continued receiving memory growth and missing samples fail the endurance check", () => {
  assert.throws(() => assertStableMemory(memorySamples(true), 7, 0), /kept growing/);
  assert.throws(() => assertStableMemory(memorySamples(false), 8, 0), /not enough memory samples/);
});
