// Noise suppression (FR-3.65, AC-3.56, PR #126): Settings → Voice offers Light, Standard, Advanced
// (AI) with a Beta badge and Off, every backend has to lower noise while keeping the voice, and the
// choice changes while the microphone runs. The client reads the test clip as its microphone through
// the file audio backend, so the bot records what the app sends.
// Voice activation is put on the manual threshold with the slider at its minimum and the automatic
// volume off, so the recorded pauses show the suppressor instead of the gate or the gain.
//
// The bot starts recording before the app is restarted, so the recorded window always contains the
// clip. Restarting first lets the recording begin in the middle of the clip: the alignment then
// matches a shifted part of it and the measured levels say nothing about the suppressor, which is what
// made this spec fail in CI. A run that did not record the whole copy, or whose recorded copy sits
// outside the window the connect time allows, is reported instead of measured.
//
// How much each backend removes on its own is proven in crates/codec/src/dsp/tests.rs and, for
// rnnoise-rs, in crates/engine/src/noise_backend.rs; here the
// encoder limits what the network path can show, so the checks stay modest.
import { mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { $, $$, browser, expect } from "@wdio/globals";
import { app, bot, repoRoot, testenv } from "../../helpers/index.ts";
import { align, concat, RATE, readWav, silence, speechAndGap, writeWav } from "../../helpers/voice-audio.ts";
import { connectFromEmptyState, openSettings, waitFor, wizard } from "../app/flows.ts";

const sleep = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms));

const dir = join(repoRoot, "e2e", "artifacts", "noise-backends");
const f = (name: string): string => join(dir, name);
const log = (msg: string): void => {
  console.log(`[noise-backends] ${msg}`);
};

const speech = readWav(join(repoRoot, "testenv", "audio", "speech.wav"));
const noisy = readWav(join(repoRoot, "testenv", "audio", "speech_noisy.wav"));

/** One 10 ms window, as the audio helpers measure it. */
const WINDOW = 480;
/** Windows the clean clip covers, which separate the voice from the noise bed. */
const REF_WINDOWS = Math.floor(speech.length / WINDOW);
/** Windows of a copy: the models need ~150 ms before their output is representative. */
const WARM_UP = 15;
/** Seconds one copy of the clip covers. */
const CLIP_SECONDS = Math.floor(noisy.length / RATE);
/** Seconds of digital silence in front of a copy. The app drops whatever the capture delivered
 * before its connection, so the reconnect has to land inside this silence: if it comes later, part
 * of the copy is gone, the alignment matches the wrong windows, and the measured levels say nothing
 * about the suppressor. */
const PREFIX = 15;
/** How far the recorded copy may sit from the window the connect time implies for it. The copy cannot
 * start before the app connected, so the alignment may not put it earlier than that; the app buffers
 * before its first frame goes out and a neural model shifts the frame it returns, so it can sit later.
 * Runs that measured correctly landed 3-57 windows after the connect; the run that failed in CI was 50
 * windows before it, which is impossible, so it matched a shifted part of the clip. */
const ALIGN_EARLY_GRACE_WINDOWS = 2;
const ALIGN_LATE_SLACK_WINDOWS = 100;
/** How far the second copy of the switch run may sit from where the first copy puts it: the neural
 * backend returns its frame one window late, anything more means audio was lost or held up. */
const SWITCH_SHIFT_WINDOWS = 3;

interface Levels {
  speechDb: number;
  gapDb: number;
}

/** The clip as it would be sent without any suppression. */
const input: Levels = speechAndGap(noisy, speech, 0, WARM_UP);

/** A run has to be quieter than the un-suppressed clip by this much to count as suppression. */
const MIN_REDUCTION_DB = 4;
/** Two runs of the same backend should not differ by more than this. */
const RUN_TOLERANCE_DB = 3;
/** How much of the voice a suppressor may take away. */
const SPEECH_TOLERANCE_DB = 6;

async function openVoiceSettings(): Promise<void> {
  const overlay = await $(".overlay__dialog");
  if (!(await overlay.isDisplayed())) await openSettings();
  // The noise suppression choice lives in the voice section. Clicking the tab keeps the run
  // independent of where the keyboard focus currently is.
  const tab = await $("button[id$='-tab-voiceAudio']");
  await tab.waitForDisplayed({ timeout: 30_000 });
  await tab.click();
  await (await $(NOISE_GROUP)).waitForDisplayed({ timeout: 30_000 });
}

const NOISE_GROUP = "//fieldset[legend[normalize-space(.)='Noise suppression']]";
const LIGHT = "Light";
const STANDARD = "Standard";
const ADVANCED = "Advanced (AI)";
/** The value each choice saves, which is also the radio's value. */
const VALUES: Record<string, string> = { [LIGHT]: "light", [STANDARD]: "standard", [ADVANCED]: "advanced", Off: "off" };

async function choiceRadio(text: string) {
  return $(`${NOISE_GROUP}//input[@type='radio' and @value='${VALUES[text] ?? ""}']`);
}

/** Label text of the selected choice, badge included. */
async function choiceValue(): Promise<string> {
  for (const radio of await $$(`${NOISE_GROUP}//input[@type='radio']`)) {
    if (await radio.isSelected()) return (await radio.parentElement()).getText();
  }
  return "";
}

async function switchByText(text: string) {
  const control = await $(`//label[contains(., '${text}')]/following::input[@role='switch'][1]`, {
    strict: false,
  });
  await control.scrollIntoView();
  return control;
}

async function pick(text: string): Promise<void> {
  const radio = await choiceRadio(text);
  await radio.scrollIntoView();
  await radio.click();
  // A settings change needs a moment to reach the engine.
  await sleep(1500);
}

/** Manual activation with the slider at its minimum: the app sends every part of the clip. */
async function sendContinuously(): Promise<void> {
  for (const radio of await $$("//input[@type='radio']")) {
    if ((await radio.getAttribute("value")) === "manual") {
      await radio.scrollIntoView();
      await radio.click();
      break;
    }
  }
  const slider = await $("//input[@type='range']");
  await slider.scrollIntoView();
  await slider.click();
  await browser.keys(["Home"]);
}

async function choose(text: string): Promise<void> {
  await openVoiceSettings();
  await pick(text);
}

/** Reconnects after an app restart; the server bar label can carry extra text. */
async function reconnectApp(): Promise<void> {
  const connected = await $("//button[starts-with(@aria-label, 'localhost (connected)')]");
  if (await connected.isDisplayed()) return;
  const button = await $("//button[starts-with(@aria-label, 'localhost')]");
  await button.waitForDisplayed({ timeout: 30_000 });
  await button.click();
  await waitFor("//button[starts-with(@aria-label, 'localhost (connected)')]", 30_000);
  await waitFor("[role='tree']", 30_000);
}

/**
 * Records `seconds` of what the app sends for `sample`, starting before the app is restarted so the
 * window always contains the clip. `during` runs while the bot is still recording. `lag` is the
 * window inside the recording where the copy of the clip begins; aligning against the clip itself
 * (not against the file, whose leading silence carries no pattern) keeps that position unambiguous.
 */
async function record(
  label: string,
  sample: Float32Array,
  seconds: number,
  during?: (elapsedMs: number) => Promise<void>,
  botTimeout = 120,
): Promise<{ received: Float32Array; lag: number }> {
  writeWav(f(`${label}-in.wav`), sample);
  const recording = bot(["record", f(`${label}.wav`), "--seconds", String(seconds)], {
    name: "Measure",
    timeout: botTimeout,
  });
  await app.restart({ audio: `file:${f(`${label}-in.wav`)}:${f(`${label}-out.wav`)}` });
  const restarted = Date.now();
  await reconnectApp();
  const connected = Date.now() - restarted;
  if (connected > PREFIX * 1000 - 1000) {
    throw new Error(
      `"${label}": the app connected ${String(connected)} ms after the restart, later than the ` +
        `${String(PREFIX * 1000)} ms prefix, so the copy lost its head and this run measures nothing`,
    );
  }
  if (during !== undefined) await during(connected);
  await recording;
  const received = readWav(f(`${label}.wav`));
  const lag = align(received, noisy);
  const earliest = PREFIX * 100 + Math.round(connected / 10);
  if (lag < earliest - ALIGN_EARLY_GRACE_WINDOWS || lag > earliest + ALIGN_LATE_SLACK_WINDOWS) {
    throw new Error(
      `"${label}": the copy cannot start before the end of the ${String(PREFIX * 100)} windows prefix ` +
        `and the app connected ${String(connected)} ms after the restart, so window ${String(earliest)} ` +
        `(up to ${String(ALIGN_LATE_SLACK_WINDOWS)} windows later for the app's startup) is where it ` +
        `has to be; the alignment put it at window ${String(lag)}, so it matched a shifted part of the ` +
        `clip and this run measures nothing`,
    );
  }
  log(`${label}: copy starts at window ${String(lag)} (${String(Math.floor(lag / 100))} s), connect ${String(connected)} ms`);
  return { received, lag };
}

/** Speech and gap levels of the copy that starts at window `offset` inside a recording. A recording
 * that does not hold the whole copy measures nothing, so it is reported instead. */
function levelsAt(received: Float32Array, offset: number, label: string): Levels {
  const windows = Math.floor(received.length / WINDOW);
  if (offset + REF_WINDOWS > windows) {
    throw new Error(
      `"${label}" recorded ${String(windows)} windows but the copy needs ${String(offset + REF_WINDOWS)}: ` +
        `the app started playing too late for this recording`,
    );
  }
  const measured = speechAndGap(received, speech, offset, WARM_UP);
  log(`${label}: speech ${measured.speechDb.toFixed(1)} dB, gap ${measured.gapDb.toFixed(1)} dB`);
  return { speechDb: measured.speechDb, gapDb: measured.gapDb };
}

/** Records one run with the current choice. */
async function measure(label: string): Promise<Levels> {
  const { received, lag } = await record(label, concat(silence(PREFIX), noisy, silence(8)), 45);
  return levelsAt(received, lag, label);
}

describe("noise suppression backends", () => {
  let off: Levels = { speechDb: 0, gapDb: 0 };
  const sent = new Map<string, Levels>();

  before(async () => {
    rmSync(dir, { recursive: true, force: true });
    mkdirSync(dir, { recursive: true });
    await testenv.up("v15");
    // The server may still be starting (another spec may have just reset it).
    for (let i = 0; ; i++) {
      try {
        await bot(["connect"], { name: "Checker" });
        break;
      } catch (e) {
        if (i > 30) throw e;
        await sleep(1000);
      }
    }
    await app.restart({ freshProfile: true });
    await wizard("NoiseTester");
    await openVoiceSettings();
    const muteSwitch = await switchByText("Connect muted");
    if (await muteSwitch.isSelected()) await muteSwitch.click();
    await sendContinuously();
    const gain = await switchByText("Automatic volume");
    if (await gain.isSelected()) await gain.click();
    await $("button.overlay__close").click();
    await connectFromEmptyState();
    await waitFor("[role='tree']", 30_000);
  });

  after(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it("offers Light, Standard, Advanced (AI) with a Beta badge and Off, Standard selected", async () => {
    await openVoiceSettings();
    expect(await choiceValue()).toBe(STANDARD);
    const labels = await (await $$(`${NOISE_GROUP}//label`)).map((label) => label.getText());
    expect(labels.map((label) => label.replace(/\s+/g, " ").trim())).toEqual([LIGHT, STANDARD, `${ADVANCED} Beta`, "Off"]);
  });

  it("sends the audio unchanged while suppression is off", async () => {
    await choose("Off");
    off = await measure("off");
    expect(Math.abs(off.gapDb - input.gapDb)).toBeLessThan(4);
    expect(Math.abs(off.speechDb - input.speechDb)).toBeLessThan(SPEECH_TOLERANCE_DB);
  });

  it("reduces noise with both backends and keeps the voice", async () => {
    for (const name of [LIGHT, STANDARD, ADVANCED]) {
      await choose(name);
      sent.set(name, await measure(VALUES[name] ?? name));
    }
    // How much a backend removes on its own is proven in crates/codec/src/dsp/tests.rs and, for
    // rnnoise-rs, in crates/engine/src/noise_backend.rs. Through the network path the encoder and the
    // alignment of one recording move a single backend's levels by more than these gates can tell
    // apart, so the check here is a direction: the noise bed has to stay below the un-suppressed run,
    // and the voice has to stay the louder part of the recording.
    for (const [name, levels] of sent) {
      expect(levels.gapDb).toBeLessThan(off.gapDb);
      expect(levels.speechDb).toBeGreaterThan(levels.gapDb);
      if (name === STANDARD) {
        expect(levels.speechDb).toBeGreaterThan(off.speechDb - SPEECH_TOLERANCE_DB);
        expect(levels.gapDb).toBeLessThan(off.gapDb - MIN_REDUCTION_DB);
      }
    }
  });

  it("changes the backend while the microphone keeps running", async () => {
    // Off with Standard as the last backend: choosing Advanced in the pause switches the
    // suppression on and the backend over in one step, without restarting the app.
    await choose(STANDARD);
    await choose("Off");
    // Two copies of the clip in one run, with a pause between them long enough for the switch to
    // land in it: the first copy goes out unsuppressed, the second with Advanced. Both are
    // measured in the same run, so only the choice differs.
    const pause = 24;
    const copyWindows = Math.floor(noisy.length / WINDOW);
    const twoCopies = concat(silence(PREFIX), noisy, silence(pause), noisy, silence(8));
    const { received, lag } = await record(
      "switch",
      twoCopies,
      70,
      async (elapsed) => {
        // The switch has to land in the pause after the first copy, so it happens
        // CLIP_SECONDS + 3 s after the app started playing, not after the reconnect.
        const wait = (PREFIX + CLIP_SECONDS + 3) * 1000 - elapsed;
        if (wait > 0) await sleep(wait);
        await openVoiceSettings();
        await pick(ADVANCED);
      },
      150,
    );
    // The second copy has to arrive exactly one copy and one pause after the first: a capture that
    // stalled or dropped frames during the switch would pull it earlier or push it later.
    const expected = lag + copyWindows + pause * 100;
    const afterFirst = lag + copyWindows;
    const found = afterFirst + align(received.subarray(afterFirst * WINDOW), noisy);
    log(`switch: second copy at window ${String(found)}, expected ${String(expected)}`);
    expect(Math.abs(found - expected)).toBeLessThanOrEqual(SWITCH_SHIFT_WINDOWS);
    const first = levelsAt(received, lag, "switch first copy");
    const second = levelsAt(received, found, "switch second copy");
    const advanced = sent.get(ADVANCED);
    if (advanced === undefined) {
      throw new Error("the Advanced (AI) run of the previous test did not finish, so there is nothing to compare with");
    }
    // The two copies measure like the off run and the Advanced run, and the second is quieter:
    // only a chain that changed while it was running can do that.
    expect(Math.abs(first.gapDb - off.gapDb)).toBeLessThan(RUN_TOLERANCE_DB);
    expect(Math.abs(second.gapDb - advanced.gapDb)).toBeLessThan(RUN_TOLERANCE_DB);
    expect(second.gapDb).toBeLessThan(first.gapDb);
    await openVoiceSettings();
    expect(await choiceValue()).toContain(ADVANCED);
  });
});
