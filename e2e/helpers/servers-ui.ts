import { spawn, type ChildProcess } from "node:child_process";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { browser } from "@wdio/globals";
import { profile } from "./app.ts";
import { bin, testenvDir } from "./paths.ts";
import { run, runOk } from "./run.ts";
import { finishWizardWithDefaults } from "./wizard-util.ts";

// UI helpers for the server specs (AC-1.4 – AC-1.6).

/** Replaces a React-controlled input's value (WebDriver's clear doesn't fire onChange). */
export async function typeInto(selector: string, value: string): Promise<void> {
  const input = browser.$(selector);
  await input.click();
  await browser.keys(["Control", "a"]);
  await browser.keys(["Backspace"]);
  if (value !== "") await input.addValue(value);
}

/** The input labelled `text` inside the open dialog (ids come from React's useId). */
export async function fieldId(text: string): Promise<string> {
  const label = browser.$(`//label[normalize-space()="${text}"]`);
  await label.waitForExist();
  const id = await label.getAttribute("for");
  if (!id) throw new Error(`label "${text}" has no for=`);
  return `[id="${id}"]`;
}

export async function fill(label: string, value: string): Promise<void> {
  await typeInto(await fieldId(label), value);
}

export function dialogTitle(text: string) {
  return browser.$(
    `//h2[contains(@class,"servers-dialog__title") and normalize-space()="${text}"]`,
  );
}

export function button(text: string) {
  return browser.$(`//button[normalize-space()="${text}"]`);
}

/** Runs the first-start wizard: new identity → default mic → voice activation. */
export async function completeWizard(name: string): Promise<void> {
  await fill("Name", name);
  await button("Next").click();
  await finishWizardWithDefaults();
}

/** Server bar button whose accessible name starts with `name` (status suffix allowed). */
export function serverButton(name: string) {
  return browser.$(`//ul[@aria-label="Saved servers"]//button[starts-with(@aria-label,"${name}")]`);
}

export async function waitServerStatus(name: string, status: string | null): Promise<void> {
  const want = status ? `${name} (${status})` : name;
  await browser.waitUntil(
    async () => (await serverButton(name).getAttribute("aria-label")) === want,
    {
      timeout: 20_000,
      timeoutMsg: `server bar never showed "${want}"`,
    },
  );
}

/** SHA-256 fingerprint (lowercase hex, no separators) of a testenv certificate set. */
export async function certFingerprint(set: "A" | "B"): Promise<string> {
  const { stdout } = await runOk("openssl", [
    "x509",
    "-noout",
    "-fingerprint",
    "-sha256",
    "-in",
    join(testenvDir, "certs", set, "cert.pem"),
  ]);
  return normalizeFingerprint(stdout.split("=")[1] ?? "");
}

export function normalizeFingerprint(text: string): string {
  return text.toLowerCase().replace(/[^0-9a-f]/g, "");
}

/** Every file below the profile whose bytes contain `needle`. */
export function filesContaining(needle: string): string[] {
  const hits: string[] = [];
  const bytes = Buffer.from(needle);
  const walk = (dir: string) => {
    for (const entry of readdirSync(dir)) {
      const path = join(dir, entry);
      const st = statSync(path);
      if (st.isDirectory()) walk(path);
      else if (st.isFile() && readFileSync(path).includes(bytes)) hits.push(path);
    }
  };
  walk(profile());
  return hits;
}

/** Mumcord's Secret Service entries as printed by secret-tool (attributes + secrets). */
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

/** Environment matching the app instance tauri-driver starts (same profile, same bus). */
export function appEnv(): NodeJS.ProcessEnv {
  return {
    ...process.env,
    XDG_DATA_HOME: join(profile(), "data"),
    XDG_CONFIG_HOME: join(profile(), "config"),
    XDG_CACHE_HOME: join(profile(), "cache"),
    MUMCORD_AUDIO_BACKEND: "null",
  };
}

/** Launches the app binary with a link, like a browser handing a mumble:// URL to the handler. */
export function launchWithLink(link: string): ChildProcess {
  return spawn(bin.app, [link], { env: appEnv(), stdio: ["ignore", "inherit", "inherit"] });
}
