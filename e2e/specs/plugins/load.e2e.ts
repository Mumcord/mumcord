// AC-6.24 (spec 06): voice stays smooth with the five official plugins active and 20 bots in the
// channel. Alice talks for two minutes (speech file as microphone) while a bot plays a steady tone
// to her, once with `--no-plugins` (baseline) and once with Poll, Dice roller, Echolings, Mumbot
// Music and EVE fit running, on the same profile, server and bots. The engine's audio health
// (jitter buffer, concealed frames, dropouts, late frames) and the glitches in Alice's playback of
// the tone are compared against the baseline; a bot records what it hears of Alice in both runs
// for the manual listening check (artifacts/plugin-load/b-*.wav).
import { mkdirSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { $, $$, browser, expect } from "@wdio/globals";
import { app, registry, repoRoot, testenv } from "../../helpers/index.ts";
import { joinChannel, ScriptBot } from "../../helpers/chat-ui.ts";
import { openSettingsGroup, turnOnDeveloperMode } from "../../helpers/settings-ui.ts";
import { activity, RATE, readWav, writeWav } from "../../helpers/voice-audio.ts";
import { addAndConnect, completeWizard, ownChannel, reconnect } from "../../helpers/voice-ui.ts";

const CHANNEL = "Lobby";
const ME = "Alice";
const BOTS = 20;
const PLUGINS = ["Poll", "Dice roller", "Echolings", "Mumbot Music", "EVE fit"];
const TALK_SECONDS = 120;
const TONE_HZ = 440;
const TONE_AMPLITUDE = 0.2;
/** Samples per 10 ms block of the glitch analysis. */
const BLOCK = RATE / 100;
/**
 * "Within normal variance" (AC-6.24) as numbers. One 20 ms packet is the smallest step the
 * jitter buffer adapts by; 60 frames are 0.5 % of the 12,000 10 ms frames of two minutes.
 */
const JITTER_MARGIN_MS = 20;
const FRAME_MARGIN = 60;
const GLITCH_MARGIN = 0.005;
/** Share of 100 ms windows in which the bot hears Alice may differ by this many points. */
const ACTIVITY_MARGIN = 5;

const dir = join(repoRoot, "e2e", "artifacts", "plugin-load");
const f = (name: string) => join(dir, name);
const prompt = "//div[contains(concat(' ',@class,' '),' plugin-prompt ')]";
const pluginTab = (name: string) =>
  $(`//button[@role='tab'][contains(@class,'plugins-tab')][starts-with(normalize-space(.),'${name}')]`);
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

interface AudioHealth {
  dropouts: number;
  lateFrames: number;
  concealedFrames: number;
  jitterBufferMs: number;
  processingMaxUs: number;
}
interface ConnectionStats {
  audio: AudioHealth | null;
}
interface PluginsOverview {
  noPlugins: boolean;
  plugins: { name: string; status: { kind: string } }[];
}

interface RunResult {
  label: string;
  jitterMeanMs: number;
  jitterMaxMs: number;
  concealed: number;
  dropouts: number;
  lateFrames: number;
  processingMaxUs: number;
  playbackBlocks: number;
  playbackGlitches: number;
  /** Percent of 100 ms windows in which the recording bot hears Alice. */
  heardPercent: number;
}

async function invoke<T>(command: string, args: object = {}): Promise<T> {
  return browser.execute(
    (c: string, a: object) => {
      const w = window as unknown as {
        __TAURI_INTERNALS__: { invoke: (c: string, a: object) => Promise<unknown> };
      };
      return w.__TAURI_INTERNALS__.invoke(c, a);
    },
    command,
    args,
  ) as Promise<T>;
}

function sine(seconds: number): Float32Array {
  const out = new Float32Array(seconds * RATE);
  for (let i = 0; i < out.length; i++) out[i] = TONE_AMPLITUDE * Math.sin((2 * Math.PI * TONE_HZ * i) / RATE);
  return out;
}

/**
 * Glitched 10 ms blocks in a recording of the steady tone, after its first clean block. A sine
 * satisfies x[n+1] + x[n-1] = 2 cos(ω) x[n]; the residual stays at the codec's noise floor for
 * clean audio and jumps at every gap, splice or concealment. Levels are relative to the tone's
 * median level, so the output volume does not matter.
 */
function toneGlitches(x: Float32Array, seconds: number): { blocks: number; glitches: number } {
  const k = 2 * Math.cos((2 * Math.PI * TONE_HZ) / RATE);
  const rmsOf: number[] = [];
  const residualOf: number[] = [];
  for (let b = 0; (b + 1) * BLOCK <= x.length; b++) {
    let s = 0;
    let r = 0;
    for (let i = b * BLOCK; i < (b + 1) * BLOCK; i++) {
      s += x[i] * x[i];
      if (i > b * BLOCK && i + 1 < (b + 1) * BLOCK) r = Math.max(r, Math.abs(x[i + 1] + x[i - 1] - k * x[i]));
    }
    rmsOf.push(Math.sqrt(s / BLOCK));
    residualOf.push(r);
  }
  const loud = rmsOf.filter((v) => v > 0.01).sort((a, b) => a - b);
  if (loud.length === 0) return { blocks: 0, glitches: 0 };
  const level = loud[Math.floor(loud.length / 2)];
  const click = (level * Math.SQRT2) / 4;
  const quiet = level / 2;
  const first = rmsOf.findIndex((v, i) => v >= quiet && residualOf[i] <= click);
  // Skip the first second (jitter buffer settling) and stop before the tone fades out.
  const from = first + 100;
  const to = Math.min(rmsOf.length, from + seconds * 100);
  let glitches = 0;
  for (let b = from; b < to; b++) if (rmsOf[b] < quiet || residualOf[b] > click) glitches++;
  return { blocks: to - from, glitches };
}

async function audioHealth(serverId: number): Promise<AudioHealth> {
  const stats = await invoke<ConnectionStats | null>("connection_stats", { serverId });
  if (stats?.audio == null) throw new Error("no audio health while connected");
  return stats.audio;
}

describe("Plugins: voice under load (AC-6.24)", () => {
  let fixtures: registry.FixtureRegistry;
  const bots: ScriptBot[] = [];
  let recorder: ScriptBot;
  let peer: ScriptBot;

  async function install(name: string): Promise<void> {
    await pluginTab("Browse").click();
    const button = $(`button[aria-label='Install ${name}']`);
    await button.waitForExist({ timeout: 30_000 });
    await button.scrollIntoView({ block: "center" });
    await button.waitForClickable({ timeout: 30_000 });
    await button.click();
    const confirm = $(`${prompt}//button[normalize-space(.)='Install']`);
    await confirm.waitForClickable({ timeout: 60_000 });
    // Every optional permission on: talking events and member badges are the load under test.
    for (const box of await $$(`${prompt}//label[contains(@class,'perm-label')]//input[@type='checkbox']`)) {
      if (!(await box.isSelected())) await box.click();
    }
    await confirm.click();
    await expect($(".plugin-toast")).toHaveText(expect.stringContaining(`${name} installed`), { wait: 60_000 });
  }

  /** Restarts the app with `args`, lets Alice talk for two minutes and measures. */
  async function measure(label: string, args: string[]): Promise<RunResult> {
    await app.restart({ args, audio: `file:${f("mic.wav")}:${f(`a-${label}.wav`)}` });
    await reconnect();
    if ((await ownChannel()) !== CHANNEL) await joinChannel(CHANNEL);
    const overview = await invoke<PluginsOverview>("plugins_list");
    if (args.includes("--no-plugins")) {
      expect(overview.noPlugins).toBe(true);
    } else {
      await browser.waitUntil(
        async () =>
          (await invoke<PluginsOverview>("plugins_list")).plugins.filter((p) => p.status.kind === "running")
            .length === PLUGINS.length,
        { timeout: 30_000, timeoutMsg: "not all five plugins are running" },
      );
    }
    const serverId = await invoke<number | null>("voice_server_get");
    if (serverId === null) throw new Error("no voice server");
    // Join sounds and the jitter buffer settle first.
    await sleep(5_000);

    const playing = peer.run(`play ${f("tone.wav")}`, (TALK_SECONDS + 60) * 1000);
    const recording = recorder.run(
      `record ${f(`b-${label}.wav`)} --seconds ${String(TALK_SECONDS)} --from ${ME}`,
      (TALK_SECONDS + 60) * 1000,
    );
    await sleep(2_000);
    const start = await audioHealth(serverId);
    const jitter: number[] = [];
    let end = start;
    for (let t = 0; t < TALK_SECONDS - 5; t += 5) {
      await sleep(5_000);
      end = await audioHealth(serverId);
      jitter.push(end.jitterBufferMs);
    }
    await Promise.all([playing, recording]);

    const playback = toneGlitches(readWav(f(`a-${label}.wav`)), TALK_SECONDS - 10);
    const heard = activity(readWav(f(`b-${label}.wav`)), -45);
    const result: RunResult = {
      label,
      jitterMeanMs: jitter.reduce((a, b) => a + b, 0) / jitter.length,
      jitterMaxMs: Math.max(...jitter),
      concealed: end.concealedFrames - start.concealedFrames,
      dropouts: end.dropouts - start.dropouts,
      lateFrames: end.lateFrames - start.lateFrames,
      processingMaxUs: end.processingMaxUs,
      playbackBlocks: playback.blocks,
      playbackGlitches: playback.glitches,
      heardPercent: (100 * heard.count) / Math.max(1, heard.total),
    };
    console.log(`[AC-6.24] ${JSON.stringify(result)}`);
    expect(playback.blocks).toBeGreaterThan((TALK_SECONDS - 20) * 100);
    expect(result.heardPercent).toBeGreaterThan(30);
    return result;
  }

  before(async function () {
    this.timeout(20 * 60_000);
    mkdirSync(dir, { recursive: true });
    // Microphone: speech, long enough for setup and both runs (the file starts with each launch).
    const speech = readWav(join(repoRoot, "testenv", "audio", "speech.wav"));
    const mic = new Float32Array(6 * 60 * RATE);
    for (let i = 0; i < mic.length; i++) mic[i] = speech[i % speech.length];
    writeWav(f("mic.wav"), mic);
    writeWav(f("tone.wav"), sine(TALK_SECONDS + 10));

    await testenv.up("v15");
    fixtures = await registry.start();
    const eveFit = readdirSync(join(fixtures.workDir, "staged")).find((n) => n.startsWith("eve-fit-"));
    if (eveFit === undefined) throw new Error("eve-fit was not built (scripts/e2e-plugin-registry.sh)");
    await fixtures.publish(eveFit.replace(/\.mcplugin$/, ""));
    await app.restart({ env: fixtures.env(), audio: `file:${f("mic.wav")}:${f("a-setup.wav")}` });
    await completeWizard(ME);
    await addAndConnect();
    await turnOnDeveloperMode();
    await openSettingsGroup("Plugins");
    await $("//button[normalize-space(.)='Browse plugins']").click();
    for (const name of PLUGINS) await install(name);
    await browser.keys("Escape");

    recorder = new ScriptBot({ name: "Rec", timeout: 60 });
    peer = new ScriptBot({ name: "Peer", timeout: 60 });
    bots.push(recorder, peer);
    for (let i = bots.length + 1; i <= BOTS; i++) bots.push(new ScriptBot({ name: `Load${String(i)}`, timeout: 60 }));
    for (const bot of bots) {
      await bot.connected();
      await bot.run(`join ${CHANNEL}`);
    }
  });

  after(() => {
    for (const bot of bots) bot.stop();
    return fixtures.stop();
  });

  it("AC-6.24: five plugins and 20 bots leave the voice statistics within normal variance", async function () {
    this.timeout(15 * 60_000);
    const baseline = await measure("baseline", ["--no-plugins"]);
    const loaded = await measure("plugins", []);
    const margin = Math.ceil(GLITCH_MARGIN * baseline.playbackBlocks);
    expect(loaded.jitterMeanMs).toBeLessThanOrEqual(baseline.jitterMeanMs + JITTER_MARGIN_MS);
    expect(loaded.concealed).toBeLessThanOrEqual(baseline.concealed + FRAME_MARGIN);
    expect(loaded.dropouts + loaded.lateFrames).toBeLessThanOrEqual(
      baseline.dropouts + baseline.lateFrames + FRAME_MARGIN,
    );
    expect(loaded.playbackGlitches).toBeLessThanOrEqual(baseline.playbackGlitches + margin);
    expect(Math.abs(loaded.heardPercent - baseline.heardPercent)).toBeLessThanOrEqual(ACTIVITY_MARGIN);
  });
});
