// Noise suppression backends (spec 23, AC-23.5, AC-23.7, AC-23.8, AC-23.11): the settings UI offers
// the three backends, only sonora-ns has its own strength setting, rnnoise-rs needs the user's
// acceptance, and every backend has to lower noise while keeping the voice. The client reads the test
// clip as its microphone through the file audio backend, so the bot records what the app sends.
// Voice activation is put on the manual threshold with the slider at its minimum and the automatic
// volume off, so the recorded pauses show the suppressor instead of the gate or the gain.
// How much each backend removes on its own is verified in crates/codec/src/dsp/tests.rs; here the
// Opus encoder limits the measurable floor, so the assertions stay modest.
import { mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { $, $$, browser, expect } from "@wdio/globals";
import { app, bot, repoRoot, testenv } from "../../helpers/index.ts";
import { align, concat, readWav, rms, silence, speechAndGap, writeWav } from "../../helpers/voice-audio.ts";
import { connectFromEmptyState, openSettings, waitFor, wizard } from "../app/flows.ts";

const sleep = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms));

const dir = join(repoRoot, "e2e", "artifacts", "noise-backends");
const f = (name: string): string => join(dir, name);
const log = (msg: string): void => {
  console.log(`[noise-backends] ${msg}`);
};

const speechPath = join(repoRoot, "testenv", "audio", "speech.wav");
const noisyPath = join(repoRoot, "testenv", "audio", "speech_noisy.wav");
const speech = readWav(speechPath);
const noisy = readWav(noisyPath);
// The clip as it would be sent without any suppression.
const input = speechAndGap(noisy, speech, 0, 0);

interface Levels {
  speechDb: number;
  gapDb: number;
}

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
  const input = await $(`//label[contains(., '${text}')]/following::input[@role='switch'][1]`, {
    strict: false,
  });
  await input.scrollIntoView();
  return input;
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
  await openVoiceSettings();
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

/** Restarts the app on the test clip (the file backend starts the clip again) and records the send. */
async function measure(label: string, backend?: string): Promise<Levels> {
  if (backend !== undefined) await selectBackend(backend);
  writeWav(f("in.wav"), concat(silence(8), noisy, silence(60)));
  await app.restart({ audio: `file:${f("in.wav")}:${f("out.wav")}` });
  await reconnectApp();
  await bot(["record", f(`${label}.wav`), "--seconds", "34"], { name: "Measure", timeout: 120 });
  const received = readWav(f(`${label}.wav`));
  const result = speechAndGap(received, speech, align(received, speech), 0);
  log(`${label}: speech ${result.speechDb.toFixed(1)} dB, gap ${result.gapDb.toFixed(1)} dB`);
  return { speechDb: result.speechDb, gapDb: result.gapDb };
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

describe("noise suppression backends", () => {
  let off: Levels = { speechDb: 0, gapDb: 0 };

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
    // The baseline: the clip as the app sends it with nothing suppressed.
    const suppression = await switchByText("Noise suppression");
    if (await suppression.isSelected()) await suppression.click();
    await $("button.overlay__close").click();
    await connectFromEmptyState();
    await waitFor("[role='tree']", 30_000);
    off = await measure("off");
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
    expect(await (await switchByText("Noise suppression")).isSelected()).toBe(false);
    expect(Math.abs(off.gapDb - input.gapDb)).toBeLessThan(4);
    expect(Math.abs(off.speechDb - input.speechDb)).toBeLessThan(6);
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
    // The engine keeps the refused library out of the frame path, so nnnoiseless stays as the
    // fallback. The refused run has to match the fallback, not the un-suppressed baseline.
    // What the refused backend does is proven in the engine test; here the settings gate is checked
    // through the UI, and the audio only has to arrive with the fallback suppressor.
    const refused = await measure("rnnoise-refused");
    expect(refused.gapDb).toBeLessThan(off.gapDb);
    await openVoiceSettings();
    const again = await switchByText("accept running this unreviewed");
    await again.click();
    expect(await again.isSelected()).toBe(true);
    expect(await (await $("//p[contains(., 'Not active until you accept')]")).isDisplayed()).toBe(false);
    const accepted = await measure("rnnoise-accepted");
    expect(accepted.gapDb).toBeLessThan(refused.gapDb - 3);
    expect(accepted.speechDb).toBeGreaterThan(off.speechDb - 6);
  });

  it("reduces noise with every backend and keeps the voice", async () => {
    // The floor the path adds by itself: pure silence through the encoder.
    writeWav(f("in.wav"), silence(60));
    await app.restart({ audio: `file:${f("in.wav")}:${f("out.wav")}` });
    await reconnectApp();
    await bot(["record", f("floor.wav"), "--seconds", "20"], { name: "Floor", timeout: 90 });
    const floor = readWav(f("floor.wav"));
    const floorDb = 10 * Math.log10(rms(floor, 48000 * 6, 48000 * 18));
    log(`floor (silence): ${floorDb.toFixed(1)} dB`);
    await setSuppression(true);
    for (const name of ["nnnoiseless", "rnnoise-rs", "sonora-ns"]) {
      await selectBackend(name);
      if (name === "sonora-ns") await pick(await levelBox(), "Very high");
      const sent = await measure(name);
      expect(sent.speechDb).toBeGreaterThan(off.speechDb - 6);
      // The neural models are far above the encoder floor, so the recorded pauses show their work.
      // sonora-ns removes about 17 dB on this clip on its own, which lands close to the floor the
      // Opus encoder adds, so only the direction can be checked here. Its strength is measured in
      // crates/codec/src/dsp/tests.rs (`strength_setting_changes_what_the_chain_sends`: low 4.9 dB, moderate 11.3 dB, very high 21.2 dB).
      expect(sent.gapDb).toBeLessThan(off.gapDb - (name === "sonora-ns" ? 1 : 3));
    }
  });

  it("changes the backend while the microphone keeps running", async () => {
    await selectBackend("nnnoiseless");
    const first = await measure("switch-first");
    expect(first.gapDb).toBeLessThan(off.gapDb - 3);
    await selectBackend("sonora-ns");
    const second = await measure("switch-second");
    expect(second.gapDb).toBeLessThan(off.gapDb - 1);
    // The app restarted while measuring, so the settings have to be opened again.
    await openVoiceSettings();
    expect(await backendValue()).toContain("sonora-ns");
  });
});
