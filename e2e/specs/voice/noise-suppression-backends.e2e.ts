// Noise suppression backends (spec 23, AC-23.5, AC-23.7, AC-23.8, AC-23.11): the settings UI offers
// the three backends, only sonora-ns has its own strength setting, rnnoise-rs needs the user's
// acceptance, and every backend has to lower noise while keeping the voice. The client reads the test
// clip as its microphone through the file audio backend, so the bot records what the app sends.
// Voice activation is put on the manual threshold with the slider at its minimum and the automatic
// volume off, so the recorded pauses show the suppressor instead of the gate or the gain.
//
// The bot starts recording before the app is restarted, so the recorded window always contains the
// clip. Restarting first lets the recording begin in the middle of the clip: the alignment then
// matches a shifted part of it and the measured levels say nothing about the suppressor, which is what
// made this spec fail in CI. A run that did not record the whole copy is reported instead of measured.
//
// How much each backend removes on its own is proven in crates/codec/src/dsp/tests.rs; here the
// encoder limits what the network path can show, so the checks stay modest.
import { mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { $, $$, browser, expect } from "@wdio/globals";
import { app, bot, repoRoot, testenv } from "../../helpers/index.ts";
import { align, concat, readWav, silence, speechAndGap, writeWav } from "../../helpers/voice-audio.ts";
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
/** Seconds of digital silence in front of a copy, so every run starts from the same quiet state. */
const PREFIX = 4;

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
  // The backend, the strength and the acceptance live in the voice section. Clicking the tab keeps
  // the run independent of where the keyboard focus currently is.
  const tab = await $("button[id$='-tab-voice']");
  await tab.waitForDisplayed({ timeout: 30_000 });
  await tab.click();
  await (await $("//label[contains(., 'Noise suppression backend')]")).waitForDisplayed({ timeout: 30_000 });
}

async function backendBox() {
  return $("//label[contains(., 'Noise suppression backend')]/following::button[@role='combobox'][1]");
}

async function levelBox() {
  return $("//label[contains(., 'Suppression strength')]/following::button[@role='combobox'][1]");
}

async function backendValue(): Promise<string> {
  return browser.execute((element: HTMLElement) => element.textContent, await backendBox());
}

async function switchByText(text: string) {
  const control = await $(`//label[contains(., '${text}')]/following::input[@role='switch'][1]`, {
    strict: false,
  });
  await control.scrollIntoView();
  return control;
}

async function pick(box: Awaited<ReturnType<typeof backendBox>>, text: string): Promise<void> {
  await box.click();
  const option = await $(`//div[@role='option' and contains(., '${text}')]`);
  await option.waitForDisplayed({ timeout: 10_000 });
  await option.click();
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

async function setSuppression(on: boolean): Promise<void> {
  await openVoiceSettings();
  const control = await switchByText("Noise suppression");
  if ((await control.isSelected()) !== on) await control.click();
}

async function selectBackend(text: string): Promise<void> {
  await openVoiceSettings();
  await pick(await backendBox(), text);
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
 * window inside the recording where `sample` begins.
 */
async function record(
  label: string,
  sample: Float32Array,
  seconds: number,
  during?: () => Promise<void>,
): Promise<{ received: Float32Array; lag: number }> {
  writeWav(f(`${label}-in.wav`), sample);
  const recording = bot(["record", f(`${label}.wav`), "--seconds", String(seconds)], {
    name: "Measure",
    timeout: 120,
  });
  await app.restart({ audio: `file:${f(`${label}-in.wav`)}:${f(`${label}-out.wav`)}` });
  await reconnectApp();
  if (during !== undefined) await during();
  await recording;
  const received = readWav(f(`${label}.wav`));
  return { received, lag: align(received, sample) };
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

/** Records one run with `backend` selected. */
async function measure(label: string, backend?: string): Promise<Levels> {
  if (backend !== undefined) await selectBackend(backend);
  const { received, lag } = await record(label, concat(silence(PREFIX), noisy, silence(14)), 40);
  return levelsAt(received, lag + PREFIX * 100, label);
}

describe("noise suppression backends", () => {
  let off: Levels = { speechDb: 0, gapDb: 0 };
  let refused: Levels = { speechDb: 0, gapDb: 0 };
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
    const suppression = await switchByText("Noise suppression");
    if (await suppression.isSelected()) await suppression.click();
    await $("button.overlay__close").click();
    await connectFromEmptyState();
    await waitFor("[role='tree']", 30_000);
  });

  after(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it("shows nnnoiseless as the default and offers all three backends", async () => {
    await openVoiceSettings();
    expect(await backendValue()).toContain("nnnoiseless");
    await (await backendBox()).click();
    const labels = await (await $$("//div[@role='option']")).map((option) => option.getText());
    expect(labels).toContain("nnnoiseless");
    expect(labels).toContain("rnnoise-rs");
    expect(labels).toContain("sonora-ns");
  });

  it("shows settings only for the backend that has them", async () => {
    await openVoiceSettings();
    for (const name of ["nnnoiseless", "rnnoise-rs"]) {
      await selectBackend(name);
      expect(await $$("//label[contains(., 'Suppression strength')]").then((l) => l.length)).toBe(0);
    }
    await selectBackend("sonora-ns");
    expect(await (await levelBox()).isDisplayed()).toBe(true);
  });

  it("sends the audio unchanged while suppression is off", async () => {
    await openVoiceSettings();
    expect(await (await switchByText("Noise suppression")).isSelected()).toBe(false);
    off = await measure("off");
    expect(Math.abs(off.gapDb - input.gapDb)).toBeLessThan(4);
    expect(Math.abs(off.speechDb - input.speechDb)).toBeLessThan(SPEECH_TOLERANCE_DB);
  });

  it("warns for rnnoise-rs and does not run it before the acceptance", async () => {
    await setSuppression(true);
    await selectBackend("rnnoise-rs");
    const warning = await $("//p[contains(., 'rnnoise-rs')]");
    await warning.scrollIntoView();
    expect(await warning.isDisplayed()).toBe(true);
    const consent = await switchByText("accept running this unreviewed");
    expect(await consent.isSelected()).toBe(false);
    const inactive = await $("//p[contains(., 'Not active until you accept')]");
    await inactive.scrollIntoView();
    expect(await inactive.isDisplayed()).toBe(true);
    // The engine keeps the refused library out of the frame path, so the audio arrives through the
    // nnnoiseless fallback. Which backend processes a refused frame is proven in the engine test
    // (crates/engine/src/session/local.rs); here the settings gate is checked through the UI, and the
    // run has to show a working suppressor. The nnnoiseless run below has to measure like this one.
    refused = await measure("rnnoise-refused");
    expect(refused.gapDb).toBeLessThan(off.gapDb - MIN_REDUCTION_DB);
    expect(refused.speechDb).toBeGreaterThan(off.speechDb - SPEECH_TOLERANCE_DB);
    await openVoiceSettings();
    const again = await switchByText("accept running this unreviewed");
    await again.click();
    expect(await again.isSelected()).toBe(true);
    expect(await (await $("//p[contains(., 'Not active until you accept')]")).isDisplayed()).toBe(false);
  });

  it("reduces noise with every backend and keeps the voice", async () => {
    await setSuppression(true);
    for (const name of ["nnnoiseless", "rnnoise-rs", "sonora-ns"]) {
      await selectBackend(name);
      if (name === "sonora-ns") await pick(await levelBox(), "Very high");
      sent.set(name, await measure(name));
    }
    for (const levels of sent.values()) {
      expect(levels.speechDb).toBeGreaterThan(off.speechDb - SPEECH_TOLERANCE_DB);
      expect(levels.gapDb).toBeLessThan(off.gapDb - MIN_REDUCTION_DB);
    }
    // nnnoiseless measuring like the refused run shows that the refused run used the fallback, and
    // rnnoise-rs, accepted by now, must be doing more than the fallback (the codec test measures
    // 66 dB against nnnoiseless' 13.6 dB on this clip), so its run has to be quieter.
    expect(Math.abs((sent.get("nnnoiseless") as Levels).gapDb - refused.gapDb)).toBeLessThan(
      RUN_TOLERANCE_DB,
    );
    expect((sent.get("rnnoise-rs") as Levels).gapDb).toBeLessThan(refused.gapDb);
  });

  it("changes the backend while the microphone keeps running", async () => {
    // The strength stays on Very high, so the second copy measures like the sonora run above.
    await selectBackend("sonora-ns");
    await pick(await levelBox(), "Very high");
    await selectBackend("nnnoiseless");
    // Two copies of the clip in one run, with a pause between them long enough for the switch to
    // land in it: the first copy goes out with nnnoiseless, the second with sonora-ns, without
    // restarting the app. Both are measured in the same run, so only the backend differs.
    const pause = 24;
    const copyWindows = Math.floor(noisy.length / WINDOW);
    const twoCopies = concat(silence(PREFIX), noisy, silence(pause), noisy, silence(8));
    const { received, lag } = await record("switch", twoCopies, 80, async () => {
      // The first copy is on its way; the switch has to land in the pause after it.
      await sleep(15_000);
      await openVoiceSettings();
      await pick(await backendBox(), "sonora-ns");
    });
    const start = lag + PREFIX * 100;
    const first = levelsAt(received, start, "switch first copy");
    const second = levelsAt(received, start + copyWindows + pause * 100, "switch second copy");
    // The two copies measure like the two backends, and they are further apart than the run noise:
    // only a backend that changed while the chain was running can do that.
    expect(Math.abs(first.gapDb - (sent.get("nnnoiseless") as Levels).gapDb)).toBeLessThan(RUN_TOLERANCE_DB);
    expect(Math.abs(second.gapDb - (sent.get("sonora-ns") as Levels).gapDb)).toBeLessThan(RUN_TOLERANCE_DB);
    expect(Math.abs(first.gapDb - second.gapDb)).toBeGreaterThan(2);
    await openVoiceSettings();
    expect(await backendValue()).toContain("sonora-ns");
  });
});
