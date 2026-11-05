// Sound from plugin web content (spec 09 FR-9.43): AC-9.24 with the `test-surface` stage page and
// its 440 Hz "Play frame tone" button.
//
//   - Without a click inside the frame the page gets no sound (`tone-auto: blocked`, silence,
//     no mute button).
//   - A click inside the frame starts it (`tone-click: ok`, the tone is heard) and the chrome
//     shows "Mute sounds from Test Surface".
//   - The mute button silences a tone that is playing and every later one.
//
// The frame's sound is WebKit's own output (GStreamer → PulseAudio), not the app's audio backend,
// so the spec runs PulseAudio with the null sink `e2e-sink` in place of PipeWire (as AC-7.66
// does) and records that sink's monitor. PipeWire comes back afterwards.
import { type ChildProcess, spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { $, browser, expect } from "@wdio/globals";
import { pluginenv, pwapps } from "../../helpers/index.ts";
import { act, FRAME_TONE_HZ, setUp, toneLevel, waitLine } from "../../helpers/plugin-sound.ts";
import { readWav } from "../../helpers/voice-audio.ts";

const SILENT_DB = -60;
const STAGE_FRAME = ".plugin-stage iframe.plugin-webview__frame";
const muteButton = () => $("button[aria-label='Mute sounds from Test Surface']");
const unmuteButton = () => $("button[aria-label='Unmute sounds from Test Surface']");
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** A genuine click on the page's "Play frame tone" button. */
async function clickToneInFrame(): Promise<void> {
  await browser.switchFrame($(STAGE_FRAME));
  try {
    const tone = $("#tone");
    await tone.waitForClickable({ timeout: 10_000 });
    await tone.click();
  } finally {
    await browser.switchFrame(null);
  }
}

describe("plugins 1.1: sound from web content (AC-9.24)", function () {
  this.timeout(10 * 60_000);
  this.bail(true);

  const dir = mkdtempSync(join(tmpdir(), "mumcord-frame-sound-"));
  const recording = join(dir, "sink.wav");
  const hadPipeWire = pwapps.pipewireRunning();
  let recorder: ChildProcess | undefined;

  const samples = () => readWav(recording).length;
  /** The frame tone's level in what the sink played from sample `from` on. */
  const levelSince = (from: number) => toneLevel(readWav(recording).slice(from), FRAME_TONE_HZ);

  before(async function () {
    this.timeout(20 * 60_000);
    if (hadPipeWire) await pwapps.pulseOnly();
    await setUp();
    recorder = spawn(
      "parec",
      [
        "--device=e2e-sink.monitor",
        "--rate=48000",
        "--channels=1",
        "--format=s16le",
        "--file-format=wav",
        recording,
      ],
      { stdio: "ignore" },
    );
    await act("Start stage", "start-stage: ok");
    await $(STAGE_FRAME).waitForExist({ timeout: 15_000, timeoutMsg: "the stage frame did not open" });
    await waitLine("ready: stage");
  });

  after(async () => {
    recorder?.kill();
    await pluginenv.stopPluginFixtures();
    if (hadPipeWire && !pwapps.pipewireRunning()) await pwapps.pipewireBack();
    rmSync(dir, { recursive: true, force: true });
  });

  it("AC-9.24: without a click inside the frame the page plays nothing", async () => {
    const from = samples();
    await act("Frame tone without click", "tone: ok");
    await waitLine("stage/tone-auto: blocked");
    await sleep(2500);
    expect(levelSince(from)).toBeLessThan(SILENT_DB);
    expect(await muteButton().isExisting()).toBe(false);
  });

  it("AC-9.24: a click inside the frame starts its sound and shows the mute button", async () => {
    const from = samples();
    await clickToneInFrame();
    await waitLine("stage/tone-click: ok");
    await muteButton().waitForDisplayed({ timeout: 5000 });
    await sleep(1500);
    const level = levelSince(from);
    console.log(`[frame sound] clicked tone ${level.toFixed(1)} dB`);
    expect(level).toBeGreaterThan(-40);
    // The 2 s tone ends before the next case.
    await sleep(1500);
  });

  it("AC-9.24: the chrome mute button silences the frame", async () => {
    await clickToneInFrame();
    await waitLine("stage/tone-click: ok");
    await muteButton().click();
    await unmuteButton().waitForDisplayed({ timeout: 5000 });
    // What was still playing stops; leave a moment for the sink's buffer.
    await sleep(500);
    const from = samples();
    await sleep(2000);
    expect(levelSince(from)).toBeLessThan(SILENT_DB);

    // A new click while muted stays silent too.
    const again = samples();
    await clickToneInFrame();
    await waitLine("stage/tone-click: blocked");
    await sleep(2500);
    expect(levelSince(again)).toBeLessThan(SILENT_DB);
  });
});
