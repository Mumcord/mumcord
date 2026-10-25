// Shared keyboard-only drivers for the C7 app specs (AC-1.24–1.27). Every interaction goes through
// WebDriver key actions; `browser.execute` is only used to READ state (focus, DOM), never to act.
import { spawn, type ChildProcess } from "node:child_process";
import { join } from "node:path";
import { browser } from "@wdio/globals";
import type { Result } from "axe-core";
import { axe, repoRoot } from "../../helpers/index.ts";
import { run } from "../../helpers/run.ts";

export const SERVER = "localhost:64738";

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

export async function waitFor(selector: string, timeout = 20_000): Promise<void> {
  await browser.$(selector).waitForExist({ timeout, timeoutMsg: `${selector} did not appear` });
}

export async function waitGone(selector: string, timeout = 20_000): Promise<void> {
  await browser.$(selector).waitForExist({ timeout, reverse: true, timeoutMsg: `${selector} did not go away` });
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

/** Wizard: create identity `name`, keep defaults, uncheck the mumble:// handler, Finish. */
export async function wizard(name: string, onStep?: (step: number) => Promise<void>): Promise<void> {
  await waitFor("form.wizard-step");
  await onStep?.(1);
  await tabTo((f) => f.tag === "input" && f.type === "text", "identity name field");
  await typeText(name);
  await browser.keys(["Enter"]);
  await waitText("p.wizard-progress", "2", 30_000);
  await onStep?.(2);
  await tabTo((f) => f.tag === "button" && f.cls.includes("button--primary"), "wizard Next");
  await browser.keys(["Enter"]);
  await waitText("p.wizard-progress", "3");
  await onStep?.(3);
  const box = await browser.execute(() => {
    const c = document.querySelector<HTMLInputElement>("form.wizard-step input[type=checkbox]");
    return c ? c.checked : null;
  });
  if (box === true) {
    await tabTo((f) => f.tag === "input" && f.type === "checkbox", "mumble:// checkbox");
    await browser.keys([" "]);
  }
  await tabTo((f) => f.tag === "button" && f.cls.includes("button--primary"), "wizard Finish");
  await browser.keys(["Enter"]);
  await waitFor(".shell", 30_000);
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

/** In the open settings panel, Tabs to the radio group containing `value` and arrows to it. */
export async function chooseRadio(value: string): Promise<void> {
  await tabTo((f) => f.tag === "input" && f.type === "radio" && groupHas(value), `radio group with ${value}`);
  for (const key of ["ArrowDown", "ArrowUp"]) {
    for (let i = 0; i < 4; i++) {
      if ((await focus()).value === value) return;
      await browser.keys([key]);
    }
  }
  if ((await focus()).value !== value) throw new Error(`radio ${value} not reachable with arrow keys`);
}

async function groupHas(value: string): Promise<boolean> {
  return browser.execute((v: string) => {
    const e = document.activeElement as HTMLInputElement;
    if (e.type !== "radio") return false;
    return Array.from(document.querySelectorAll<HTMLInputElement>(`input[type=radio][name="${e.name}"]`)).some((r) => r.value === v);
  }, value);
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
