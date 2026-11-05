// Plugin sound effects (spec 09 §4.10, FR-9.65 … FR-9.68): AC-9.23 with the `test-surface`
// fixture plugin and its bundled 660 Hz beep.
//
//   - The beep plays on A: the debug audio meter (`plugin_sound_meter`) shows output and A's
//     speakers (file audio backend) carry it; the bot Bob in the same channel records nothing.
//   - While the bot Talker talks it is ducked (FR-9.67: −12 dB).
//   - While A is deafened it is silent; with "Mute plugin sounds" on it is silent.
//   - Playing a file that is not declared in the manifest returns `not-found`.
//
// Frame sound (AC-9.24) is in sound-frame.e2e.ts.
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { $, browser, expect } from "@wdio/globals";
import { app, bot, botInBackground, pluginenv, type BackgroundBot } from "../../helpers/index.ts";
import {
  act,
  BEEP_HZ,
  closeSettings,
  panelButton,
  panelLines,
  setUp,
  toneLevel,
} from "../../helpers/plugin-sound.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";
import { RATE, readWav, silence, wavSamples, writeWav } from "../../helpers/voice-audio.ts";

/** Talker's voice: a tone far from the beep, so the beep is measured on its own. */
const TALK_HZ = 1320;
/** Below this a recording counts as silent at the beep's frequency. */
const SILENT_DB = -60;

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const deafenButton = () => $("button[aria-label='Deafen']");

function sine(hz: number, seconds: number, amplitude: number): Float32Array {
  const out = new Float32Array(Math.round(seconds * RATE));
  for (let i = 0; i < out.length; i++) out[i] = amplitude * Math.sin((2 * Math.PI * hz * i) / RATE);
  return out;
}

describe("plugins 1.1: plugin sound effects (AC-9.23)", function () {
  this.timeout(10 * 60_000);
  this.bail(true);

  const dir = mkdtempSync(join(tmpdir(), "mumcord-plugin-sound-"));
  const out = join(dir, "out.wav");
  const talk = join(dir, "talk.wav");
  let talker: BackgroundBot | undefined;
  /** The beep's level on A's speakers with nobody talking. */
  let plainDb = -200;

  /** Clicks "Play sound" and returns what A's speakers played from then until the beep ended. */
  async function playAndCapture(): Promise<Float32Array> {
    const s0 = wavSamples(out);
    await act("Play sound", "play-sound: ok");
    // The beep is 0.3 s; the file backend refreshes the WAV header once a second.
    await sleep(1500);
    return readWav(out).slice(s0, wavSamples(out));
  }

  before(async function () {
    this.timeout(20 * 60_000);
    writeWav(join(dir, "in.wav"), silence(1));
    writeWav(talk, sine(TALK_HZ, 6, 0.5));
    await setUp(`file:${join(dir, "in.wav")}:${out}`);
  });

  after(async () => {
    talker?.stop();
    await pluginenv.stopPluginFixtures();
    rmSync(dir, { recursive: true, force: true });
  });

  it("AC-9.23: the bundled sound plays on A only, the debug meter shows it", async () => {
    // Bob sits in A's channel (the root) and records while A plays the sound three times.
    const recording = join(dir, "bob.wav");
    const bob = bot(["record", recording, "--seconds", "6"], { name: "Bob" });
    await sleep(1500);
    let meterPeak = 0;
    const s0 = wavSamples(out);
    for (let i = 0; i < 3; i++) {
      await act("Play sound", "play-sound: ok");
      const until = Date.now() + 600;
      while (Date.now() < until) {
        meterPeak = Math.max(meterPeak, await app.invoke<number>("plugin_sound_meter"));
      }
    }
    await sleep(1500);
    const played = readWav(out).slice(s0, wavSamples(out));
    await bob;
    plainDb = toneLevel(played, BEEP_HZ);
    const atBob = toneLevel(readWav(recording), BEEP_HZ);
    console.log(`[sound] A ${plainDb.toFixed(1)} dB, Bob ${atBob.toFixed(1)} dB, meter ${meterPeak.toFixed(3)}`);
    expect(meterPeak).toBeGreaterThan(0.05);
    expect(plainDb).toBeGreaterThan(-30);
    expect(atBob).toBeLessThan(SILENT_DB);
  });

  it("AC-9.23: while somebody talks the sound is ducked", async () => {
    talker = botInBackground(["script"], { name: "Talker", timeout: 20 });
    await sleep(2000);
    talker.send(`play ${talk}`);
    // Talker's voice reaches A and marks them talking before the beep starts.
    await sleep(1500);
    const ducked = toneLevel(await playAndCapture(), BEEP_HZ);
    console.log(`[sound] plain ${plainDb.toFixed(1)} dB, ducked ${ducked.toFixed(1)} dB`);
    expect(plainDb - ducked).toBeGreaterThan(8);
    expect(plainDb - ducked).toBeLessThan(16);
    talker.stop();
    talker = undefined;
    // Ducking ends once nobody talks.
    await sleep(2000);
    const again = toneLevel(await playAndCapture(), BEEP_HZ);
    expect(Math.abs(plainDb - again)).toBeLessThan(3);
  });

  it("AC-9.23: while A is deafened the sound is silent", async () => {
    await deafenButton().click();
    await expect(deafenButton()).toHaveAttribute("aria-pressed", "true");
    // Mumcord's own "deafened" sound plays first; it is not a plugin sound.
    await sleep(2000);
    const level = toneLevel(await playAndCapture(), BEEP_HZ);
    await deafenButton().click();
    await expect(deafenButton()).toHaveAttribute("aria-pressed", "false");
    expect(level).toBeLessThan(SILENT_DB);
    expect(await app.invoke<number>("plugin_sound_meter")).toBe(0);
  });

  it("AC-9.23: the global plugin-sound mute silences it", async () => {
    const toggleMute = async (on: boolean) => {
      await openSettingsGroup("Plugins");
      const mute = $("//input[@role='switch'][@id=//label[.='Mute plugin sounds']/@for]");
      await mute.waitForEnabled({ timeout: 10_000 });
      if ((await mute.isSelected()) !== on) await mute.click();
      await browser.waitUntil(async () => (await mute.isSelected()) === on, {
        timeout: 5000,
        timeoutMsg: "Mute plugin sounds did not switch",
      });
      await closeSettings();
    };
    await toggleMute(true);
    const muted = toneLevel(await playAndCapture(), BEEP_HZ);
    await toggleMute(false);
    const unmuted = toneLevel(await playAndCapture(), BEEP_HZ);
    expect(muted).toBeLessThan(SILENT_DB);
    expect(Math.abs(plainDb - unmuted)).toBeLessThan(3);
  });

  it("AC-9.23: playing an undeclared file returns not-found", async () => {
    await panelButton("Play undeclared sound");
    // `play-missing: not-found`; until the fixture prints only the WIT error name, its whole
    // debug text (`error { code: 9, name: "not-found", … }`).
    await browser.waitUntil(
      async () => (await panelLines()).some((l) => /^play-missing: .*not-found/.test(l)),
      { timeout: 15_000, timeoutMsg: "play-missing never reported not-found" },
    );
  });
});
