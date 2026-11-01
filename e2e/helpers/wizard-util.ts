// Shared steps and file helpers for the wizard acceptance specs (AC-1.1 – AC-1.3).
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { $, browser } from "@wdio/globals";
import { artifactsDir, repoRoot } from "./paths.ts";
import { run, runOk } from "./run.ts";

export const wizardArtifacts = join(artifactsDir, "wizard");

/**
 * Finds a button by its visible text (exact match after trimming). The wizard's footer buttons
 * carry a decorative arrow ("Next →", "← Back"); it is ignored.
 */
export function button(text: string) {
  return $(
    `//button[normalize-space(.)="${text}" or normalize-space(.)="${text} →" or normalize-space(.)="← ${text}"]`,
  );
}

/** Finds a form control through its `<label for>` text. */
export async function labelled(text: string) {
  const label = $(`//label[normalize-space(.)="${text}"]`);
  await label.waitForExist();
  const id = await label.getAttribute("for");
  return $(`[id="${id ?? ""}"]`);
}

/** Text of the whole page (to look for names and status lines). */
export async function pageText(): Promise<string> {
  return $("body").getText();
}

export async function waitForText(text: string, timeout = 15_000): Promise<void> {
  await browser.waitUntil(async () => (await pageText()).includes(text), {
    timeout,
    timeoutMsg: `text ${JSON.stringify(text)} never appeared`,
  });
}

/** The main window is up once the server bar's "Add a server" button exists. */
export function mainWindowMarker() {
  return $('button[aria-label="Add a server"]');
}

/**
 * Picks `path` in the native GTK "Open File" chooser (rfd's default title). The chooser opens
 * asynchronously and Xvfb has no window manager, so keys sent before it is mapped and focused hit
 * an unrealized widget and are lost (seen on loaded CI runners); wait for it and focus it first.
 */
export async function chooseFileInNativeDialog(path: string): Promise<void> {
  let chooser = "";
  await browser.waitUntil(
    async () => {
      const found = await run("xdotool", ["search", "--onlyvisible", "--name", "^Open File$"]);
      chooser = found.stdout.trim().split("\n")[0] ?? "";
      return chooser !== "";
    },
    { timeout: 20_000, interval: 250, timeoutMsg: "native file chooser never opened" },
  );
  await runOk("xdotool", ["windowfocus", "--sync", chooser]);
  // Ctrl+L opens the location field; give GTK a moment to show it before typing.
  await runOk("xdotool", ["key", "ctrl+l"]);
  await browser.pause(300);
  await runOk("xdotool", ["type", "--delay", "15", path]);
  await browser.pause(300);
  await runOk("xdotool", ["key", "Return"]);
}

// ---- Setup wizard (#159) ----
//
// First start: Welcome → [Import from Mumble] → Identity → Voice → Setup style → [Custom only:
// Private messages → Connections → Features → Appearance → Startup and updates] → All set.
// Rerun from Settings → About (#149): the same without import and identity.

/** Rail names of the steps, as the step header shows them ("STEP n OF m · <name>"). */
export const WIZARD_STEP_NAMES = {
  welcome: "Welcome",
  import: "Import from Mumble",
  identity: "Identity",
  voice: "Voice",
  setupStyle: "Setup style",
  privateMessages: "Private messages",
  connections: "Connections",
  features: "Features",
  appearance: "Appearance",
  startup: "Startup and updates",
  summary: "All set",
} as const;
export type WizardStepName = keyof typeof WIZARD_STEP_NAMES;

/** The Custom-only steps in order. */
export const CUSTOM_STEPS: readonly WizardStepName[] = [
  "privateMessages",
  "connections",
  "features",
  "appearance",
  "startup",
];

/** The four cards of the setup style step (their radio values). */
export type SetupStyle = "everythingOn" | "balanced" | "maximumPrivacy" | "custom";
export type WizardPreset = Exclude<SetupStyle, "custom">;

/** The current step's header line, e.g. "Step 3 of 5 · Voice" (empty when no step shows). */
export async function wizardEyebrow(): Promise<string> {
  const eyebrow = $("form.wizard-step p.wizard-eyebrow");
  return (await eyebrow.isExisting()) ? (await eyebrow.getText()).trim() : "";
}

/** Waits until step `step` shows; resolves with its header line ("Step n of m · Name"). */
export async function waitForWizardStep(step: WizardStepName, timeout = 15_000): Promise<string> {
  // CSS may upper-case the header, so compare without case.
  const suffix = `· ${WIZARD_STEP_NAMES[step]}`.toLowerCase();
  let line = "";
  await browser.waitUntil(
    async () => {
      line = await wizardEyebrow();
      return line.toLowerCase().endsWith(suffix);
    },
    {
      timeout,
      timeoutMsg: `wizard step ${step} never showed (last header: ${JSON.stringify(line)})`,
    },
  );
  return line;
}

/** The primary (submit) button of the shown step. */
export function wizardPrimary() {
  return $("form.wizard-step .wizard-footer button[type='submit']");
}

/** Welcome → "Let's get started". */
export async function passWelcome(): Promise<void> {
  await waitForWizardStep("welcome", 30_000);
  await wizardPrimary().click();
}

/** On the identity step: a new identity named `name`, then Next. */
export async function createIdentity(name: string): Promise<void> {
  await waitForWizardStep("identity", 30_000);
  const nameInput = $("input[autocomplete='nickname']");
  await nameInput.waitForDisplayed({ timeout: 10_000 });
  await nameInput.setValue(name);
  await wizardPrimary().click();
}

/**
 * After Next on the identity step: waits for the voice step, keeps its defaults (voice
 * activation) and moves on to the setup style step.
 */
export async function voiceToSetupStyle(): Promise<void> {
  // Leaving the identity step generates the RSA-2048 identity; in the unoptimised debug build the
  // random prime search takes anywhere from about 1 s to well over 15 s on a 2-core runner.
  await waitForWizardStep("voice", 120_000);
  await wizardPrimary().click();
  await waitForWizardStep("setupStyle");
}

/** The radio of setup style card `style`. */
export function setupStyleRadio(style: SetupStyle) {
  return $(`form.wizard-step input.preset-card__radio[value='${style}']`);
}

/** On the setup style step: selects the card `style`. */
export async function chooseSetupStyle(style: SetupStyle): Promise<void> {
  const radio = setupStyleRadio(style);
  await radio.waitForExist({ timeout: 10_000 });
  // The card's label takes the click, as a user would click the card.
  await $(`form.wizard-step label.preset-card[data-preset='${style}']`).click();
  await browser.waitUntil(async () => radio.isSelected(), {
    timeout: 5_000,
    timeoutMsg: `setup style ${style} never became selected`,
  });
}

/** On the setup style step: `preset` → "Finish setup" → summary → "Start Mumcord" → main window. */
export async function finishWithPreset(preset: WizardPreset): Promise<void> {
  await chooseSetupStyle(preset);
  await button("Finish setup").click();
  await waitForWizardStep("summary");
  await button("Start Mumcord").click();
  await mainWindowMarker().waitForExist({ timeout: 30_000 });
}

/**
 * On the setup style step: Custom → Next through the five Custom steps (running `onStep` on each
 * before Next) → summary.
 */
export async function walkCustomSteps(
  onStep?: (step: WizardStepName) => Promise<void>,
): Promise<void> {
  await chooseSetupStyle("custom");
  await wizardPrimary().click();
  for (const step of CUSTOM_STEPS) {
    await waitForWizardStep(step);
    await onStep?.(step);
    await wizardPrimary().click();
  }
  await waitForWizardStep("summary");
}

/** A switch (`role="switch"` input) by the text of its label. */
export function switchLabelled(text: string) {
  return $(`//input[@role='switch'][@id=//label[normalize-space(.)="${text}"]/@for]`);
}

/**
 * The whole first-start wizard on a fresh profile: Welcome, a new identity `name`, voice
 * defaults, `preset`, summary, main window.
 *
 * The default is "Everything on" because it keeps the behaviour the specs were written against:
 * anyone may send private messages (Balanced allows only friends and the own channel), and the
 * extra sharing it turns on (now playing, now gaming) has nothing to report in the test session.
 */
export async function runFirstStartWizard(
  name: string,
  preset: WizardPreset = "everythingOn",
): Promise<void> {
  await passWelcome();
  await createIdentity(name);
  await voiceToSetupStyle();
  await finishWithPreset(preset);
}

/** All Mumcord entries in the Secret Service, as printed by `secret-tool search --all`. */
export async function keyringDump(): Promise<string> {
  const r = await run("secret-tool", [
    "search",
    "--all",
    "--unlock",
    "service",
    "app.mumcord.client",
  ]);
  return r.stdout + r.stderr;
}

// ---- WAV (48 kHz mono 16-bit PCM) ----

export function readWavPcm(path: string): Int16Array {
  const buf = readFileSync(path);
  let off = 12;
  while (off + 8 <= buf.length) {
    const id = buf.toString("ascii", off, off + 4);
    let size = buf.readUInt32LE(off + 4);
    if (id === "data") {
      // The file backend refreshes the header only once a second; take what is on disk.
      size = Math.min(size === 0 ? Infinity : size, buf.length - off - 8);
      const start = off + 8;
      const bytes = buf.subarray(start, start + (size - (size % 2)));
      return new Int16Array(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.length));
    }
    off += 8 + size + (size % 2);
  }
  throw new Error(`${path}: no data chunk`);
}

export function writeWavPcm(path: string, pcm: Int16Array, rate = 48_000): void {
  const header = Buffer.alloc(44);
  header.write("RIFF", 0, "ascii");
  header.writeUInt32LE(36 + pcm.byteLength, 4);
  header.write("WAVEfmt ", 8, "ascii");
  header.writeUInt32LE(16, 16);
  header.writeUInt16LE(1, 20);
  header.writeUInt16LE(1, 22);
  header.writeUInt32LE(rate, 24);
  header.writeUInt32LE(rate * 2, 28);
  header.writeUInt16LE(2, 32);
  header.writeUInt16LE(16, 34);
  header.write("data", 36, "ascii");
  header.writeUInt32LE(pcm.byteLength, 40);
  writeFileSync(
    path,
    Buffer.concat([header, Buffer.from(pcm.buffer, pcm.byteOffset, pcm.byteLength)]),
  );
}

/** testenv speech sample repeated `times` times (the file backend starts it when the app starts). */
export function longSpeechWav(path: string, times: number): void {
  const speech = readWavPcm(join(repoRoot, "testenv", "audio", "speech.wav"));
  const out = new Int16Array(speech.length * times);
  for (let i = 0; i < times; i++) out.set(speech, i * speech.length);
  writeWavPcm(path, out);
}

/** RMS in dBFS of each 100 ms window. */
export function windowDb(pcm: Int16Array, rate = 48_000): number[] {
  const n = rate / 10;
  const out: number[] = [];
  for (let i = 0; i + n <= pcm.length; i += n) {
    let sum = 0;
    for (let j = i; j < i + n; j++) sum += (pcm[j] / 32768) ** 2;
    const rms = Math.sqrt(sum / n);
    out.push(rms > 0 ? 20 * Math.log10(rms) : -120);
  }
  return out;
}

export function ensureArtifacts(): void {
  mkdirSync(wizardArtifacts, { recursive: true });
}

/** Prints the app's log files of the current profile (for diagnosing failures). */
export function dumpAppLogs(profileRoot: string): void {
  const dir = join(profileRoot, "data", "app.mumcord.client", "logs");
  if (!existsSync(dir)) {
    console.log(`no app logs in ${dir}`);
    return;
  }
  for (const f of readdirSync(dir)) console.log(`--- ${f}\n${readFileSync(join(dir, f), "utf8")}`);
}
