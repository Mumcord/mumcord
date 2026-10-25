// Shared steps and file helpers for the wizard acceptance specs (AC-1.1 – AC-1.3).
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { $, browser } from "@wdio/globals";
import { artifactsDir, repoRoot } from "./paths.ts";
import { run, runOk } from "./run.ts";

export const wizardArtifacts = join(artifactsDir, "wizard");

/** Finds a button by its visible text (exact match after trimming). */
export function button(text: string) {
  return $(`//button[normalize-space(.)="${text}"]`);
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

/** Number of first-start wizard steps; the last one is the talk mode step with Finish. */
export const WIZARD_STEPS = 4;

/** The wizard's progress line for step `n`. */
export function wizardStep(n: number): string {
  return `Step ${String(n)} of ${String(WIZARD_STEPS)}`;
}

/**
 * After Next on the identity step: Next through the steps in between with their defaults until
 * the last step shows. Every spec walks the wizard through here, so a new step needs one edit.
 */
export async function nextToLastWizardStep(): Promise<void> {
  for (let step = 2; step < WIZARD_STEPS; step++) {
    // Leaving step 1 generates the RSA-2048 identity; in the unoptimised debug build the random
    // prime search takes anywhere from about 1 s to well over 15 s on a 2-core runner.
    await waitForText(wizardStep(step), step === 2 ? 120_000 : 15_000);
    await button("Next").click();
  }
  await waitForText(wizardStep(WIZARD_STEPS));
}

/** After Next on the identity step: all remaining steps with defaults, Finish, main window. */
export async function finishWizardWithDefaults(): Promise<void> {
  await nextToLastWizardStep();
  await button("Finish").click();
  await mainWindowMarker().waitForExist({ timeout: 30_000 });
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
