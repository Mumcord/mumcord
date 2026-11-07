// Shared steps of the documentation screenshot walkthroughs (issue #565): a 2x display of its own
// for client A, dark and light pictures of one screen, and scrolling the settings pages.
import { spawn, type ChildProcess } from "node:child_process";
import { randomBytes } from "node:crypto";
import { cpSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { $, browser } from "@wdio/globals";
import { appPid } from "../helpers/app.ts";
import { artifactsDir, bin, repoRoot } from "../helpers/paths.ts";
import { runOk } from "../helpers/run.ts";

const SHOT_DIR = join(repoRoot, "docs", "images", "screenshots");

/** A display of A's own, big enough for a 1440×900 window at 2x. */
export const HIDPI_DISPLAY = `:${String(150 + Number(process.env.MUMCORD_E2E_RUN ?? 0))}`;

/** App environment that puts the app on the 2x display. */
export const HIDPI_ENV: Record<string, string> = { DISPLAY: HIDPI_DISPLAY, GDK_SCALE: "2" };

export const sleep = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms));

/** Starts the 2x display; returns the X server process to stop after the run. */
export async function startHidpiDisplay(): Promise<ChildProcess> {
  mkdirSync(SHOT_DIR, { recursive: true });
  mkdirSync(artifactsDir, { recursive: true });
  const xvfb = spawn("Xvfb", [HIDPI_DISPLAY, "-screen", "0", "3200x2000x24", "-nolisten", "tcp"], {
    stdio: "ignore",
  });
  await sleep(1_500);
  return xvfb;
}

/** Runs `fn` with the X tools (xdotool) pointed at A's display. */
export async function onHidpiDisplay(fn: () => Promise<void>): Promise<void> {
  const previous = process.env.DISPLAY;
  process.env.DISPLAY = HIDPI_DISPLAY;
  try {
    await fn();
  } finally {
    process.env.DISPLAY = previous;
  }
}

async function shot(name: string): Promise<void> {
  const file = join(artifactsDir, `${name}.png`);
  await browser.saveScreenshot(file);
  cpSync(file, join(SHOT_DIR, `${name}.png`));
  console.log(`screenshot: ${join(SHOT_DIR, `${name}.png`)}`);
}

export async function closeOverlays(): Promise<void> {
  for (let i = 0; i < 3; i += 1) {
    await browser.keys("Escape");
    await sleep(300);
  }
}

/**
 * Switches dark or light through Quick settings, as a person would: a settings change made with
 * the backend command alone does not reach the open window.
 */
export async function setMode(mode: "dark" | "light"): Promise<void> {
  const gear = $("button[aria-label='Quick settings']");
  await gear.waitForClickable({ timeout: 15_000 });
  await gear.click();
  const item = $(
    `//*[@role='menuitemradio'][normalize-space(.)='${mode === "dark" ? "Dark" : "Light"}']`,
  );
  await item.waitForClickable({ timeout: 10_000 });
  await item.click();
  await sleep(500);
  if (await $(".quick-settings").isExisting()) await browser.keys("Escape");
  // Escape hands focus back to the gear, whose tooltip would then stay in the picture.
  await browser.execute(() => {
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
  });
  // Park the pointer on the empty part of the server bar, so no tooltip stays in the picture.
  await browser.action("pointer").move({ x: 24, y: 600 }).perform();
  await sleep(1_000);
}

/**
 * Takes `<name>-dark` and `<name>-light`. Before each, open menus and dialogs are closed (unless
 * `keep`), the theme is switched and `prepare` sets the screen up again.
 */
export async function both(
  name: string,
  prepare: () => Promise<void> = () => Promise.resolve(),
  keep = false,
): Promise<void> {
  for (const mode of ["dark", "light"] as const) {
    if (!keep) await closeOverlays();
    await setMode(mode);
    await prepare();
    await sleep(1_500);
    await shot(`${name}-${mode}`);
  }
}

/** Scrolls the settings page to a fraction of its height (0 = top, 1 = bottom). */
export async function scrollPage(fraction: number): Promise<void> {
  await browser.execute((f: number) => {
    let el = document.querySelector("h2.settings-page__title")?.parentElement ?? null;
    while (
      el &&
      !(
        el.scrollHeight > el.clientHeight + 10 && /auto|scroll/.test(getComputedStyle(el).overflowY)
      )
    ) {
      el = el.parentElement;
    }
    if (el) el.scrollTop = Math.round(f * (el.scrollHeight - el.clientHeight));
  }, fraction);
  await sleep(500);
}

/** Scrolls the element with the exact text `text` to the top of its scroll area. */
export async function scrollToText(text: string): Promise<void> {
  const target = $(`//*[text()[normalize-space()='${text}']]`);
  await target.waitForExist({ timeout: 10_000, timeoutMsg: `no "${text}" to scroll to` });
  await target.scrollIntoView({ block: "center" });
  await sleep(500);
}

export async function openAppsMenu(): Promise<void> {
  const button = $("button.chat-apps__button");
  await button.waitForClickable({ timeout: 20_000 });
  if ((await button.getAttribute("aria-expanded")) !== "true") await button.click();
  await $("div.chat-apps__menu").waitForDisplayed({ timeout: 10_000 });
}

/**
 * A real mumbot with modules (unlike the scripted test bots it says it is a bot), so the channel
 * tree shows the Bot label and "Bots on this server" lists it. Its config and data live in a temp
 * dir; the music key is random per run.
 */
export function moduleBot(
  name: string,
  port: number,
  modules: string[],
  apiPort: number,
): ChildProcess {
  const dir = mkdtempSync(join(tmpdir(), "mumcord-shots-bot-"));
  const config = [
    "[server]",
    'host = "127.0.0.1"',
    `port = ${String(port)}`,
    `name = "${name}"`,
    "[music]",
    `listen = "127.0.0.1:${String(apiPort)}"`,
    'channel = "Root"',
    `media_dir = "${join(repoRoot, "testenv", "mumbot", "media")}"`,
    "[[music.keys]]",
    `key = "${randomBytes(16).toString("hex")}"`,
    'label = "shots"',
    'channels = ["Root"]',
    "[history]",
    'channels = ["Root"]',
    `database = "${join(dir, "history.sqlite3")}"`,
  ].join("\n");
  writeFileSync(join(dir, "bot.toml"), config);
  const child = spawn(
    bin.bot,
    ["run", "--config", join(dir, "bot.toml"), "--modules", modules.join(",")],
    {
      env: { ...process.env, MUMBOT_DATA: dir },
      stdio: ["ignore", "ignore", "inherit"],
    },
  );
  return child;
}

/**
 * Drops files onto Alex's window. Alex runs on the HiDPI display at scale 2, so the drop goes to
 * that display and the point is in device pixels.
 */
export async function dropOnA(paths: string[]): Promise<void> {
  const rect = await browser.execute(() => {
    const r = document.querySelector(".chat")?.getBoundingClientRect();
    return r ? { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2) } : null;
  });
  if (!rect) throw new Error("no chat column to drop on");
  const script = join(repoRoot, "e2e", "helpers", "x11-drop.py");
  await runOk(
    "python3",
    ["-I", script, String(appPid("a")), String(rect.x * 2), String(rect.y * 2), ...paths],
    {
      ...process.env,
      DISPLAY: HIDPI_DISPLAY,
    },
  );
}
