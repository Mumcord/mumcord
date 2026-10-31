// AC-1.24 (join/leave sounds in the speaker output, measured in the file backend's out.wav; no TTS)
// and AC-1.25 (close while connected -> ask -> minimize + remember -> hidden, still connected;
// next close does not ask). The tray itself cannot be seen in Xvfb: a stand-in
// StatusNotifierWatcher makes the app take its "tray present -> hide window" path.
import { spawn } from "node:child_process";
import { mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { browser, expect } from "@wdio/globals";
import { app, artifactsDir, bin, bot, testenv } from "../../helpers/index.ts";
import { dumpAppLogs } from "../../helpers/wizard-util.ts";
import {
  closeWindowLikeUser,
  connectFromEmptyState,
  focus,
  joinChannel,
  startTrayHost,
  tabTo,
  waitFor,
  windowVisible,
  wizard,
} from "./flows.ts";

const RATE = 48_000;
const dir = join(artifactsDir, "c7-audio");
const inWav = join(dir, "silence-in.wav");
const outWav = join(dir, "speaker-out.wav");
const me = "C7Snd";
const peer = "C7SndPeer";
const CHANNEL = "AFK";
// v14, not v15: bots that stay connected on v15 join and leave on their own, and every one of
// them in the spec's channel would add a cue the measurements cannot tell from the spec's own.
const PORT = testenv.ports.v14;

function silenceWav(seconds: number): Buffer {
  const n = RATE * seconds;
  const b = Buffer.alloc(44 + n * 2);
  b.write("RIFF", 0);
  b.writeUInt32LE(36 + n * 2, 4);
  b.write("WAVEfmt ", 8);
  b.writeUInt32LE(16, 16);
  b.writeUInt16LE(1, 20);
  b.writeUInt16LE(1, 22);
  b.writeUInt32LE(RATE, 24);
  b.writeUInt32LE(RATE * 2, 28);
  b.writeUInt16LE(2, 32);
  b.writeUInt16LE(16, 34);
  b.write("data", 36);
  b.writeUInt32LE(n * 2, 40);
  return b;
}

/** Samples (as -1..1) of out.wav; reads the raw file so a stale header does not matter. */
function samples(): Float32Array {
  const buf = readFileSync(outWav);
  const data = buf.indexOf("data", 12) + 8;
  const n = Math.floor((buf.length - data) / 2);
  const out = new Float32Array(n);
  for (let i = 0; i < n; i++) out[i] = buf.readInt16LE(data + i * 2) / 32768;
  return out;
}

const sampleCount = () => Math.floor((statSync(outWav).size - 44) / 2);
const db = (x: number) => (x > 0 ? 20 * Math.log10(x) : -Infinity);

interface Burst {
  startMs: number;
  durationMs: number;
  peakDb: number;
  rmsDb: number;
}

/** Sound bursts (>-60 dBFS, gaps <60 ms merged) between two sample positions. */
function bursts(from: number, to: number): Burst[] {
  const s = samples().subarray(from, to);
  const win = RATE / 100; // 10 ms
  const thr = 10 ** (-60 / 20);
  const result: Burst[] = [];
  let cur: { a: number; b: number } | undefined;
  for (let w = 0; w * win < s.length; w++) {
    let peak = 0;
    for (let i = w * win; i < Math.min(s.length, (w + 1) * win); i++) peak = Math.max(peak, Math.abs(s[i]));
    if (peak > thr) {
      if (cur && w - cur.b <= 6) cur.b = w;
      else {
        if (cur) result.push(summarize(s, cur, win));
        cur = { a: w, b: w };
      }
    }
  }
  if (cur) result.push(summarize(s, cur, win));
  return result;
}

function summarize(s: Float32Array, c: { a: number; b: number }, win: number): Burst {
  let peak = 0;
  let sum = 0;
  const a = c.a * win;
  const b = Math.min(s.length, (c.b + 1) * win);
  for (let i = a; i < b; i++) {
    peak = Math.max(peak, Math.abs(s[i]));
    sum += s[i] * s[i];
  }
  return {
    startMs: Math.round((a / RATE) * 1000),
    durationMs: Math.round(((b - a) / RATE) * 1000),
    peakDb: Math.round(db(peak) * 10) / 10,
    rmsDb: Math.round(db(Math.sqrt(sum / (b - a))) * 10) / 10,
  };
}

async function settle(ms: number): Promise<number> {
  await browser.pause(ms);
  return sampleCount();
}

describe("AC-1.24 join/leave sounds", () => {
  const log: Record<string, unknown> = {};

  before(async () => {
    await testenv.up("v14");
    mkdirSync(dir, { recursive: true });
    writeFileSync(inWav, silenceWav(5));
    await app.restart({ audio: `file:${inWav}:${outWav}` });
    try {
      await wizard(me);
    } catch (e) {
      dumpAppLogs(app.profile());
      throw e;
    }
    await connectFromEmptyState(undefined, `localhost:${String(PORT)}`);
    await joinChannel(CHANNEL);
    await bot(["expect", "user", me, "--in", CHANNEL], { name: peer, port: PORT, timeout: 20 });
  });

  after(() => {
    writeFileSync(join(dir, "measurements.json"), JSON.stringify(log, null, 2));
  });

  it("is silent while nothing happens", async () => {
    const a = await settle(1500);
    const b = await settle(2500);
    log.idle = bursts(a, b);
    expect(bursts(a, b)).toEqual([]);
  });

  it("plays a short subtle sound when a bot joins and when it leaves the channel", async () => {
    const a = await settle(500);
    // Connects into Root, moves into the channel (join), stays 3 s, disconnects (leave).
    await bot(["join", CHANNEL], { name: peer, port: PORT, hold: 3, timeout: 20 });
    const b = await settle(2500);
    const found = bursts(a, b);
    log.joinLeave = found;
    expect(found.length).toBe(2);
    for (const burst of found) {
      expect(burst.peakDb).toBeLessThanOrEqual(-12);
      expect(burst.peakDb).toBeGreaterThan(-40);
      expect(burst.durationMs).toBeLessThanOrEqual(500);
    }
    // The two cues are ~3 s apart (join, hold 3 s, leave).
    expect(found[1].startMs - found[0].startMs).toBeGreaterThan(2500);
  });

  it("plays nothing for a bot that joins another channel (no TTS / no speech at any time)", async () => {
    const a = await settle(500);
    await bot(["join", "Raid"], { name: peer, port: PORT, hold: 2, timeout: 20 });
    const b = await settle(2000);
    log.otherChannel = bursts(a, b);
    expect(bursts(a, b)).toEqual([]);
    // No speech: every sound in the whole recording is a short cue, never a long utterance.
    // Cues played back to back (the explicit join's undeafen + unmute) merge into one burst of
    // ~0.5 s; a spoken utterance would last well over a second.
    const all = bursts(0, sampleCount());
    log.wholeRecording = all;
    for (const burst of all) expect(burst.durationMs).toBeLessThanOrEqual(1000);
  });
});

describe("AC-1.25 close while connected", () => {
  let tray: ReturnType<typeof spawn> | undefined;

  after(() => {
    tray?.kill();
  });

  it("asks 'Minimize to tray or quit?' and hides the window on minimize + remember", async () => {
    tray = await startTrayHost();
    expect(await windowVisible()).toBe(true);
    await closeWindowLikeUser();
    await waitFor("dialog.app-dialog[open]");
    const title = await browser.execute(() => document.querySelector("dialog.app-dialog[open] h2, dialog.app-dialog[open] h1")?.textContent ?? "");
    expect(title).toBe("Minimize to tray or quit?");
    await tabTo((f) => f.tag === "input" && f.type === "checkbox", "Remember my choice");
    await browser.keys([" "]);
    expect((await focus()).checked).toBe(true);
    await tabTo((f) => f.tag === "button" && f.text === "Minimize to tray", "Minimize to tray");
    await browser.keys(["Enter"]);
    await browser.waitUntil(async () => !(await windowVisible()), { timeout: 10_000, timeoutMsg: "window still visible" });
  });

  it("stays connected while hidden", async () => {
    await browser.pause(4000);
    expect(await windowVisible()).toBe(false);
    await bot(["expect", "user", me, "--in", CHANNEL], { name: peer, port: PORT, timeout: 15 });
  });

  it("next close hides again without asking", async () => {
    // A second launch hands over to the running instance and shows its window (single instance).
    const second = spawn(bin.app, [], { env: { ...process.env, XDG_DATA_HOME: join(app.profile(), "data"), XDG_CONFIG_HOME: join(app.profile(), "config"), XDG_CACHE_HOME: join(app.profile(), "cache") }, stdio: "ignore" });
    await browser.waitUntil(windowVisible, { timeout: 15_000, timeoutMsg: "second launch did not show the window" });
    second.kill();
    await closeWindowLikeUser();
    await browser.waitUntil(async () => !(await windowVisible()), { timeout: 10_000, timeoutMsg: "window not hidden on second close" });
    const dialog = await browser.execute(() => document.querySelector("dialog.app-dialog[open]") !== null);
    expect(dialog).toBe(false);
    await bot(["expect", "user", me, "--in", CHANNEL], { name: peer, port: PORT, timeout: 15 });
  });

  it("remembered choice survives a restart (close hides without asking)", async () => {
    await app.restart();
    await waitFor(".shell", 30_000);
    // Reconnect by keyboard: the server button in the bar.
    await browser.pause(3000);
    const connected = await browser.execute(() => document.querySelector('.server-bar__item[data-status="connected"], .server-bar__item[data-status="connecting"]') !== null);
    if (!connected) {
      // The Messages button (#173) comes before the servers in the bar.
      await tabTo((f) => f.cls.includes("server-bar__server") && !f.cls.includes("pm-button__button"), "server button");
      await browser.keys(["Enter"]);
    }
    await waitFor('.server-bar__item[data-status="connected"]', 30_000);
    await bot(["expect", "user", me], { name: peer, port: PORT, timeout: 20 });
    await closeWindowLikeUser();
    await browser.waitUntil(async () => !(await windowVisible()), { timeout: 10_000, timeoutMsg: "window not hidden after restart" });
    expect(await browser.execute(() => document.querySelector("dialog.app-dialog[open]") !== null)).toBe(false);
    await bot(["expect", "user", me], { name: peer, port: PORT, timeout: 15 });
  });
});
