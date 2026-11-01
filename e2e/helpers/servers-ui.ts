import { spawn, type ChildProcess } from "node:child_process";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { browser } from "@wdio/globals";
import { profile } from "./app.ts";
import { bin, testenvDir } from "./paths.ts";
import { run, runOk } from "./run.ts";
import { hasClass } from "./selectors.ts";
import { runFirstStartWizard } from "./wizard-util.ts";

// UI helpers for the server specs (AC-1.4 – AC-1.6) and the identity manager (issue #148).

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

/** Switches the add/edit server dialog to one of its tabs (issue #151). */
export async function dialogTab(name: "Connection" | "Identity" | "Passwords" | "Behaviour") {
  const tab = browser.$(
    `//div[@role="tablist"]//button[@role="tab" and normalize-space()="${name}"]`,
  );
  await tab.waitForClickable();
  await tab.click();
}

export function dialogTitle(text: string) {
  return browser.$(
    `//h2[contains(@class,"servers-dialog__title") and normalize-space()="${text}"]`,
  );
}

/** Title of a dialog inside the settings screen (the settings dialogs use an `h3`). */
export function settingsTitle(text: string) {
  return browser.$(`//h3[${hasClass("settings-dialog__title")} and normalize-space()="${text}"]`);
}

/** The open settings modal (the settings screen itself is a dialog too, so match by class). */
export function settingsDialog(title: string) {
  return browser.$(`//div[${hasClass("settings-dialog")}][.//h3[normalize-space()="${title}"]]`);
}

/** Types into the field of the settings dialog `title` that is labelled `labelText`. */
export async function fillIn(title: string, labelText: string, value: string): Promise<void> {
  const label = settingsDialog(title).$(`.//label[normalize-space()="${labelText}"]`);
  await label.waitForExist();
  const id = await label.getAttribute("for");
  if (!id) throw new Error(`label "${labelText}" in "${title}" has no for=`);
  await typeInto(`[id="${id}"]`, value);
}

/** A button inside the open settings dialog titled `title`. */
export function dialogButtonIn(title: string, text: string) {
  return settingsDialog(title).$(`.//button[normalize-space()="${text}"]`);
}

/** The error line inside the open settings dialog titled `title` (role="alert"). */
export async function settingsErrorIn(title: string): Promise<string> {
  const alert = settingsDialog(title).$(`.//*[contains(@class,"settings-error")]`);
  await alert.waitForDisplayed({ timeout: 15_000 });
  await browser.waitUntil(async () => (await alert.getText()) !== "", {
    timeout: 15_000,
    timeoutMsg: `no error shown in the "${title}" dialog`,
  });
  return alert.getText();
}

/** The settings screen: quick settings → "All settings…" → the section `id` (works in every language). */
export async function openSettingsSection(id: string): Promise<void> {
  // The identity dialogs are modal on top of the settings screen: when it is already open, only
  // the section changes.
  if (!(await browser.$(".overlay__dialog").isExisting())) {
    const gear = browser.$('//button[@aria-label="Quick settings"]');
    await gear.waitForClickable();
    await gear.click();
    const footer = browser.$(`//button[${hasClass("quick-settings__footer")}]`);
    await footer.waitForClickable();
    await footer.click();
  }
  const tab = browser.$(`[id$="-tab-${id}"]`);
  await tab.waitForClickable();
  await tab.click();
}

/** Opens the themed dropdown labelled `label` and picks the option with text `optionText`. */
export async function pickOption(label: string, optionText: string): Promise<void> {
  const box = browser.$(
    `//label[normalize-space()="${label}"]/following::button[@role="combobox"][1]`,
  );
  await box.waitForClickable();
  await box.click();
  const option = browser.$(
    `//div[@role="listbox"]//div[@role="option"][starts-with(normalize-space(), "${optionText}")]`,
  );
  await option.waitForDisplayed();
  await option.click();
}

/** A button in a section card's action bar (never the one inside one of its dialogs). */
export function settingsAction(text: string) {
  return browser.$(
    `//div[${hasClass("settings-group__actions")}]//button[normalize-space()="${text}"]`,
  );
}

export function button(text: string) {
  return browser.$(`//button[normalize-space()="${text}"]`);
}

/** A button whose accessible name is exactly `label` (the identity cards name their actions). */
export function ariaButton(label: string) {
  return browser.$(`//button[@aria-label="${label}"]`);
}

/**
 * Types `path` into the native file chooser whose window name starts with `title`. WebDriver
 * cannot reach native GTK dialogs, so the path goes into the location field (Ctrl+L). Without a
 * window manager the chooser is not mapped by one: wait until it is visible and focus it first,
 * otherwise the keys hit an unrealized widget and are lost (see wizard-ac-1.2).
 */
export async function choosePathInNativeDialog(title: string, path: string): Promise<void> {
  let window = "";
  await browser.waitUntil(
    async () => {
      const found = await run("xdotool", ["search", "--onlyvisible", "--name", title]);
      window = found.stdout.trim().split("\n")[0] ?? "";
      return window !== "";
    },
    { timeout: 20_000, interval: 250, timeoutMsg: `native dialog "${title}" never opened` },
  );
  await runOk("xdotool", ["windowfocus", "--sync", window]);
  await runOk("xdotool", ["key", "ctrl+l"]);
  await browser.pause(300);
  await runOk("xdotool", ["type", "--delay", "30", path]);
  await browser.pause(300);
  await runOk("xdotool", ["key", "Return"]);
}

/** Runs the first-start wizard: new identity → default mic → voice activation. */
export async function completeWizard(name: string): Promise<void> {
  await runFirstStartWizard(name);
}

/** Opens the context menu of one server icon (a script event: a WebDriver right-click leaves the
 * page unable to type into fields afterwards, see theme-language.e2e.ts). */
export async function openServerMenu(name: string): Promise<void> {
  await browser.execute((label: string) => {
    const icon = Array.from(document.querySelectorAll(".server-bar__server")).find((element) =>
      (element.getAttribute("aria-label") ?? "").startsWith(label),
    ) as HTMLElement | null;
    const box = icon?.getBoundingClientRect();
    if (!icon || !box) throw new Error(`server icon ${label} not found`);
    icon.dispatchEvent(
      new MouseEvent("contextmenu", {
        bubbles: true,
        cancelable: true,
        button: 2,
        clientX: box.left + 4,
        clientY: box.top + 4,
      }),
    );
  }, name);
  const menu = browser.$(`//div[@role="menu"][@aria-label="Options for ${name}"]`);
  await menu.waitForDisplayed({ timeout: 10_000 });
}

/** A menu item of the open context menu of `name`. */
export function menuItem(name: string, text: string) {
  return browser.$(
    `//div[@role="menu"][@aria-label="Options for ${name}"]//button[normalize-space()="${text}"]`,
  );
}

/** Server bar button whose accessible name starts with `name` (status suffix allowed). */
export function serverButton(name: string) {
  return browser.$(`//ul[@aria-label="Saved servers"]//button[starts-with(@aria-label,"${name}")]`);
}

export async function waitServerStatus(
  name: string,
  status: string | null,
  timeout = 20_000,
): Promise<void> {
  // The accessible name can carry extra parts (", voice server"), so match the prefix.
  const want = status ? `${name} (${status})` : name;
  await browser.waitUntil(
    async () => ((await serverButton(name).getAttribute("aria-label")) ?? "").startsWith(want),
    {
      timeout,
      interval: 1_000,
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
