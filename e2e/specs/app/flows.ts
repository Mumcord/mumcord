// Shared keyboard-only drivers for the C7 app specs (AC-1.24–1.27). Every interaction goes through
// WebDriver key actions; `browser.execute` is only used to READ state (focus, DOM), never to act.
import { spawn, type ChildProcess } from "node:child_process";
import { join } from "node:path";
import { browser } from "@wdio/globals";
import type { Result } from "axe-core";
import { axe, repoRoot } from "../../helpers/index.ts";
import { ports } from "../../helpers/testenv.ts";
import { run } from "../../helpers/run.ts";
import {
  CUSTOM_STEPS,
  SETUP_STYLES,
  waitForWizardStep,
  type SetupStyle,
  type WizardStepName,
} from "../../helpers/wizard-util.ts";

export const SERVER = `localhost:${String(ports.v15)}`;

export interface Focus {
  tag: string;
  id: string;
  cls: string;
  type: string;
  role: string;
  label: string;
  text: string;
  checked: boolean;
  href: string;
  value: string;
  name: string;
  /** CSS selector of the nearest landmark-ish container (for debugging). */
  where: string;
}

export async function focus(): Promise<Focus> {
  return browser.execute(() => {
    const e = document.activeElement as HTMLElement | null;
    if (!e) return { tag: "", id: "", cls: "", type: "", role: "", label: "", text: "", checked: false, href: "", value: "", name: "", where: "" };
    const box = e.closest("dialog,[role=dialog],nav,main,aside,section,form");
    return {
      tag: e.tagName.toLowerCase(),
      id: e.id,
      cls: typeof e.className === "string" ? e.className : "",
      type: e.getAttribute("type") ?? "",
      role: e.getAttribute("role") ?? "",
      label: e.getAttribute("aria-label") ?? "",
      text: (e.innerText || "").trim().slice(0, 80),
      checked: (e as Partial<HTMLInputElement>).checked === true,
      href: e.getAttribute("href") ?? "",
      value: (e as Partial<HTMLInputElement>).value ?? "",
      name: e.getAttribute("name") ?? "",
      where: box ? `${box.tagName.toLowerCase()}.${box.className}` : "",
    };
  });
}

export const describeFocus = (f: Focus): string =>
  `<${f.tag} id=${f.id} class="${f.cls}" role=${f.role} label="${f.label}" text="${f.text}"> in ${f.where}`;

/** Presses Tab (Shift+Tab when `back`) until the focused element matches, or throws. */
export async function tabTo(pred: (f: Focus) => boolean | Promise<boolean>, what: string, back = false, max = 120): Promise<Focus> {
  const seen: string[] = [];
  for (let i = 0; i < max; i++) {
    const f = await focus();
    if (await pred(f)) return f;
    seen.push(describeFocus(f));
    await browser.keys(back ? ["Shift", "Tab"] : ["Tab"]);
  }
  throw new Error(`keyboard: could not Tab to ${what}; focus path:\n${seen.slice(0, 40).join("\n")}`);
}

/** Presses `key` until the focused element matches, or throws. */
export async function pressUntil(key: string, pred: (f: Focus) => boolean, what: string, max = 40): Promise<Focus> {
  for (let i = 0; i < max; i++) {
    const f = await focus();
    if (pred(f)) return f;
    await browser.keys([key]);
  }
  throw new Error(`keyboard: ${key} never reached ${what}; ended at ${describeFocus(await focus())}`);
}

/**
 * Types text one key action per character. WebKitWebDriver drops the second of two identical
 * consecutive characters sent in one action sequence ("hello" -> "helo", also in plain inputs),
 * so each character gets its own action plus a short gap.
 */
export async function typeText(text: string): Promise<void> {
  for (const ch of text) {
    await browser.keys(ch);
    await browser.pause(30);
  }
}

// These say "something matching appeared/disappeared", not "this one element". A strict `$` (wdio 10)
// throws when several elements match, so the checks count matches through `$$`.
export async function waitFor(selector: string, timeout = 20_000): Promise<void> {
  await browser.waitUntil(async () => (await browser.$$(selector)).length > 0, {
    timeout,
    timeoutMsg: `${selector} did not appear`,
  });
}

export async function waitGone(selector: string, timeout = 20_000): Promise<void> {
  await browser.waitUntil(async () => (await browser.$$(selector)).length === 0, {
    timeout,
    timeoutMsg: `${selector} did not go away`,
  });
}

export async function waitText(selector: string, text: string, timeout = 20_000): Promise<void> {
  await browser.waitUntil(
    async () =>
      browser.execute(
        (s: string, t: string) => Array.from(document.querySelectorAll(s)).some((e) => (e as HTMLElement).innerText.includes(t)),
        selector,
        text,
      ),
    { timeout, timeoutMsg: `${selector} never showed "${text}"` },
  );
}

export async function htmlTheme(): Promise<string> {
  return browser.execute(() => document.documentElement.dataset.theme ?? "");
}

// ---------------------------------------------------------------- axe collection

export interface Finding {
  screen: string;
  id: string;
  impact: string;
  help: string;
  targets: string[];
}
export const findings: Finding[] = [];

/** Scans the current screen and records every violation (selectors included). */
export async function scan(screen: string): Promise<Result[]> {
  // Let transitions/animations settle so colour-contrast sees final colours.
  await browser.pause(400);
  const violations = await axe.scan(screen.replace(/[^\w-]+/g, "_"));
  for (const v of violations) {
    findings.push({
      screen,
      id: v.id,
      impact: v.impact ?? "?",
      help: v.help,
      targets: v.nodes.map((n) => `${n.target.join(" ")} :: ${n.failureSummary?.split("\n")[1]?.trim() ?? ""}`),
    });
  }
  return violations;
}

export function findingsReport(): string {
  return findings
    .map((f) => `[${f.screen}] ${f.impact} ${f.id} (${f.help})\n    ${f.targets.join("\n    ")}`)
    .join("\n");
}

// ---------------------------------------------------------------- flows

/** Tabs to the shown wizard step's primary button (Next, Finish setup, …) and presses Enter. */
async function wizardPrimaryByKeyboard(what: string): Promise<void> {
  await tabTo((f) => f.tag === "button" && f.cls.includes("button--primary"), what);
  await browser.keys(["Enter"]);
}

export interface WizardOptions {
  /** Setup style card to pick (default "everythingOn", see `runFirstStartWizard`). */
  style?: SetupStyle;
  /** Runs once each step shows, before the keyboard leaves it. */
  onStep?: (step: WizardStepName) => Promise<void>;
}

/**
 * First-start wizard by keyboard only (#159): Welcome, a new identity `name`, voice defaults,
 * the setup style card `style` (arrow keys, Enter), the Custom steps with their defaults when
 * `style` is "custom" (the mumble:// handler unticked on the startup step), summary, Start.
 */
export async function wizard(name: string, options: WizardOptions = {}): Promise<void> {
  const { style = "everythingOn", onStep } = options;
  await waitForWizardStep("welcome", 30_000);
  await onStep?.("welcome");
  await wizardPrimaryByKeyboard("wizard Let's get started");

  await waitForWizardStep("identity");
  await onStep?.("identity");
  await tabTo((f) => f.tag === "input" && f.type === "text", "identity name field");
  await typeText(name);
  await browser.keys(["Enter"]);

  // Leaving the identity step generates the RSA-2048 identity, which takes a while in debug builds.
  await waitForWizardStep("voice", 120_000);
  await onStep?.("voice");
  await wizardPrimaryByKeyboard("wizard Next");

  await waitForWizardStep("setupStyle");
  await onStep?.("setupStyle");
  const card = await tabTo(
    (f) => f.tag === "input" && f.type === "radio" && f.cls.includes("preset-card__radio"),
    "setup style cards",
  );
  // WebKit stops at the ends of a radio group instead of wrapping, so move towards the card.
  const order: readonly string[] = SETUP_STYLES;
  const key = order.indexOf(style) < order.indexOf(card.value) ? "ArrowUp" : "ArrowDown";
  await pressUntil(key, (f) => f.value === style, `setup style ${style}`, order.length);
  // Enter on a radio continues: a preset jumps to the summary, Custom to its first step.
  await browser.keys(["Enter"]);

  if (style === "custom") {
    for (const step of CUSTOM_STEPS) {
      await waitForWizardStep(step);
      await onStep?.(step);
      if (step === "startup") await untickLinkHandler();
      await wizardPrimaryByKeyboard(`wizard Next on ${step}`);
    }
  }

  await waitForWizardStep("summary");
  await onStep?.("summary");
  await wizardPrimaryByKeyboard("wizard Start Mumcord");
  await waitFor(".shell", 30_000);
}

/** Startup step: unticks "Open mumble:// links" by keyboard, so a test never claims the handler. */
async function untickLinkHandler(): Promise<void> {
  const box = await browser.execute(() => {
    const label = Array.from(document.querySelectorAll("form.wizard-step label")).find((l) =>
      l.textContent.includes("mumble://"),
    );
    const input = label?.querySelector<HTMLInputElement>("input[type=checkbox]") ?? null;
    return input === null ? null : { checked: input.checked, id: input.id };
  });
  if (box?.checked !== true) return;
  await tabTo((f) => f.tag === "input" && f.type === "checkbox" && f.id === box.id, "mumble:// checkbox");
  await browser.keys([" "]);
}

/** Empty state: type the address, Enter (Add and connect), trust the certificate. */
export async function connectFromEmptyState(onCert?: () => Promise<void>, address = SERVER): Promise<void> {
  await waitFor(".servers-empty input");
  await tabTo((f) => f.tag === "input" && f.where.includes("servers-empty"), "empty-state address field");
  await typeText(address);
  await browser.keys(["Enter"]);
  await waitFor("dialog[open] .servers-dialog__actions", 30_000);
  await onCert?.();
  const trust = await browser.execute(
    () => document.querySelector("dialog[open] .servers-dialog__actions button.button--primary")?.textContent ?? "",
  );
  await tabTo((f) => f.tag === "button" && f.text === trust.trim(), `cert "${trust}" button`);
  await browser.keys(["Enter"]);
  await waitFor('.server-bar__item[data-status="connected"]', 30_000);
}

/** Focuses the channel tree via the "Skip to channels" link. */
export async function focusTree(): Promise<void> {
  await tabTo((f) => f.href === "#channels", "skip link to channels");
  await browser.keys(["Enter"]);
  await tabTo((f) => f.role === "treeitem", "channel tree");
}

/** Focuses the chat via the "Skip to chat" link. */
export async function focusChat(): Promise<void> {
  await tabTo((f) => f.href === "#chat", "skip link to chat");
  await browser.keys(["Enter"]);
}

export async function treeTo(channel: string): Promise<void> {
  const isRow = (f: Focus) => f.role === "treeitem" && f.cls.includes("tree-channel");
  const name = async () =>
    browser.execute(
      () => document.activeElement?.querySelector(":scope > * .tree-channel__name, .tree-channel__name")?.textContent.trim() ?? "",
    );
  await browser.keys(["Home"]);
  for (let i = 0; i < 40; i++) {
    const f = await focus();
    if (isRow(f) && (await name()) === channel) return;
    await browser.keys(["ArrowDown"]);
  }
  throw new Error(`channel ${channel} not reachable with arrow keys`);
}

/** Keyboard-joins a channel (Shift+Enter) and opens its chat (Enter). */
export async function joinChannel(channel: string): Promise<void> {
  await focusTree();
  await treeTo(channel);
  await browser.keys(["Shift", "Enter"]);
  await browser.keys(["Enter"]);
}

export async function focusComposer(): Promise<void> {
  await tabTo((f) => f.tag === "textarea" && f.cls.includes("chat-composer__input"), "chat composer");
}

export async function openSettings(): Promise<void> {
  await tabTo((f) => f.tag === "button" && ["Quick settings", "Ajustes rápidos", "Schnelleinstellungen"].includes(f.label), "Settings button");
  await browser.keys(["Enter"]);
  await waitFor(".quick-settings");
  await browser.keys(["End", "Enter"]);
  await waitFor(".overlay__dialog");
  await tabTo((f) => f.role === "tab", "settings tabs");
}

/** Moves to settings tab `index` (0-based) with arrow keys; selection follows focus. */
export async function settingsTab(index: number): Promise<string> {
  await tabTo((f) => f.role === "tab", "settings tabs", true);
  await browser.keys(["Home"]);
  for (let i = 0; i < index; i++) await browser.keys(["ArrowDown"]);
  return (await focus()).text;
}

/**
 * Picks the UI language with the keyboard only: tabs to the searchable language picker, types
 * `search` (an English name such as "German", so no non-ASCII keys are needed) and presses Enter
 * on the first match.
 */
export async function chooseLanguage(code: string, search: string): Promise<void> {
  await tabTo((f) => f.role === "combobox" && f.cls.includes("select__input"), "language picker");
  // Focus selects the shown name so typing replaces it; after an earlier pick the field keeps focus.
  await browser.execute(() => {
    (document.activeElement as HTMLInputElement).select();
  });
  await typeText(search);
  await browser.waitUntil(
    () =>
      browser.execute((c: string) => {
        const field = document.activeElement;
        const first = document.querySelector(`#${CSS.escape(field?.getAttribute("aria-controls") ?? "")} [role=option]`);
        return field?.getAttribute("aria-activedescendant") === first?.id && first?.getAttribute("lang") === c;
      }, code),
    { timeoutMsg: `"${search}" did not list ${code} first in the language picker` },
  );
  await browser.keys(["Enter"]);
}

// ---------------------------------------------------------------- X11 / D-Bus helpers

const here = join(repoRoot, "e2e", "specs", "app");

/** Sends WM_DELETE_WINDOW to the Mumcord window (= user clicks the title-bar close button). */
export async function closeWindowLikeUser(): Promise<void> {
  const r = await run("python3", ["-I", join(here, "x11-close.py"), "Mumcord"]);
  if (r.code !== 0 || r.stdout.trim() === "0") throw new Error(`x11-close failed: ${r.stdout} ${r.stderr}`);
}

export async function windowVisible(): Promise<boolean> {
  const r = await run("xdotool", ["search", "--onlyvisible", "--name", "^Mumcord$"]);
  return r.stdout.trim().length > 0;
}

/** Starts a stand-in StatusNotifierWatcher so the app sees a tray host. */
export async function startTrayHost(): Promise<ChildProcess> {
  const child = spawn("python3", ["-I", join(here, "fake-sni-watcher.py")], { stdio: ["ignore", "pipe", "inherit"] });
  for (let i = 0; i < 50; i++) {
    const r = await run("gdbus", [
      "call", "--session", "--dest", "org.freedesktop.DBus", "--object-path", "/org/freedesktop/DBus",
      "--method", "org.freedesktop.DBus.NameHasOwner", "org.kde.StatusNotifierWatcher",
    ]);
    if (r.stdout.includes("true")) return child;
    await new Promise((res) => setTimeout(res, 100));
  }
  child.kill();
  throw new Error("fake StatusNotifierWatcher did not take the bus name");
}
