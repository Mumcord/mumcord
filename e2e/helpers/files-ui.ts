// File sharing in the running client (spec 07): test files, the real drop and file picker, the
// composer chips and the file cards. Works on whichever client `apps.onB`/`onC` made current.
import { createHash, randomBytes } from "node:crypto";
import {
  closeSync,
  createReadStream,
  mkdirSync,
  mkdtempSync,
  openSync,
  truncateSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { browser, $, expect } from "@wdio/globals";
import { appPid, type AppId } from "./app.ts";
import { repoRoot } from "./paths.ts";
import { run, runOk } from "./run.ts";

/** A new folder for the files one test offers. */
export function fileDir(): string {
  return mkdtempSync(join(tmpdir(), "mumcord-files-"));
}

/** Writes `size` random bytes (incompressible, so zips and transfers really carry them). */
export function randomFile(dir: string, name: string, size: number): string {
  const path = join(dir, name);
  const chunk = 1 << 20;
  const parts: Buffer[] = [];
  for (let left = size; left > 0; left -= chunk) parts.push(randomBytes(Math.min(chunk, left)));
  mkdirSync(dir, { recursive: true });
  writeFileSync(path, Buffer.concat(parts));
  return path;
}

/** A sparse file of `size` bytes: large offers without filling the disk or the hash time. */
export function sparseFile(dir: string, name: string, size: number): string {
  const path = join(dir, name);
  closeSync(openSync(path, "w"));
  truncateSync(path, size);
  return path;
}

/** The SHA-256 of a file as lowercase hex. */
export async function sha256(path: string): Promise<string> {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) hash.update(chunk as Buffer);
  return hash.digest("hex");
}

/**
 * Drags `paths` from "a file manager" onto the chat of client `id` with a real XDND drag
 * (helpers/x11-drop.py); the same GTK drop path as a user's drag. `leave` only hovers.
 */
export async function dropFiles(id: AppId, paths: string[], leave = false): Promise<void> {
  const rect = await browser.execute(() => {
    const chat = document.querySelector(".chat");
    if (!chat) return null;
    const r = chat.getBoundingClientRect();
    return { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2) };
  });
  if (!rect) throw new Error("no chat column to drop on");
  const script = join(repoRoot, "e2e", "helpers", "x11-drop.py");
  const args = ["-I", script, String(appPid(id)), String(rect.x), String(rect.y), ...paths];
  if (leave) args.push("--leave");
  await runOk("python3", args);
}

/**
 * Picks `path` in the system file picker the paperclip opened (GTK dialog titled `title`):
 * Ctrl+L, the path, Enter, like a keyboard user.
 */
export async function pickInFileDialog(title: string, path: string): Promise<void> {
  let dialog = "";
  await browser.waitUntil(
    async () => {
      const windows = await run("xdotool", ["search", "--class", "mumcord"]);
      for (const id of windows.stdout.split("\n").filter((line) => line !== "")) {
        const name = await run("xdotool", ["getwindowname", id]);
        if (name.stdout.trim() === title) {
          dialog = id;
          return true;
        }
      }
      return false;
    },
    { timeout: 20_000, interval: 250, timeoutMsg: `file picker "${title}" never opened` },
  );
  await runOk("xdotool", ["windowfocus", "--sync", dialog]);
  await runOk("xdotool", ["key", "ctrl+l"]);
  await browser.pause(300);
  await runOk("xdotool", ["type", "--delay", "20", path]);
  await browser.pause(300);
  await runOk("xdotool", ["key", "Return"]);
}

/**
 * Registers the current user on the shown server (quick settings → Register on this server): the
 * relay bot hands out relay tokens to registered users only.
 */
export async function registerOnServer(): Promise<void> {
  const gear = $('button[aria-label="Quick settings"]');
  await gear.waitForClickable();
  await gear.click();
  const item = $('//*[@role="menuitem"][starts-with(normalize-space(), "Register on this server")]');
  await item.waitForClickable();
  await item.click();
  const confirm = $("button=Register");
  await confirm.waitForClickable();
  await confirm.click();
  await browser.waitUntil(async () => !(await $("[role=dialog], dialog[open]").isDisplayed()), {
    timeoutMsg: "the register dialog did not close",
  });
}

/** Waits until the paperclip is enabled: file sharing works in the shown channel. */
export async function waitSharingReady(): Promise<void> {
  const clip = $("button[aria-label='Share files in this channel']");
  await clip.waitForExist({ timeout: 30_000, timeoutMsg: "no paperclip" });
  await browser.waitUntil(async () => (await clip.getAttribute("aria-disabled")) === null, {
    timeout: 60_000,
    timeoutMsg: `file sharing stays unavailable: ${String(await clip.getAttribute("data-tooltip"))}`,
  });
}

/** The chip of a file in the composer tray. */
export function chip(name: string) {
  return $(`//ul[@aria-label='Files to share']/li[.//*[normalize-space()='${name}']]`);
}

/** Waits until the chip of `name` is ready to send ("<size> · peer-to-peer"). */
export async function waitChipReady(name: string): Promise<void> {
  await chip(name).waitForExist({ timeout: 20_000, timeoutMsg: `no chip for ${name}` });
  await browser.waitUntil(async () => (await chip(name).getText()).includes("peer-to-peer"), {
    timeout: 120_000,
    timeoutMsg: `the chip of ${name} never became ready`,
  });
}

/** Presses the composer's Send button (sends the ready chips). */
export async function send(): Promise<void> {
  const button = $("button[aria-label='Send']");
  await button.waitForEnabled({ timeout: 20_000 });
  // A notification toast can sit on top of the button; the click still means the button.
  await button.click().catch(async () => {
    await browser.execute((el: HTMLElement) => {
      el.click();
    }, await button);
  });
}

/** The file card of `name` (own or received) in the open chat. */
export function card(name: string) {
  return $(`//div[@role='group'][starts-with(@aria-label,'${name},')]`);
}

/** A button of the card of `name` by its visible text. */
export function cardButton(name: string, text: string) {
  return card(name).$(`.//button[normalize-space()='${text}']`);
}

/** Waits until the card of `name` contains `text`. */
export async function waitCardText(name: string, text: string, timeout = 60_000): Promise<void> {
  let seen = "";
  await card(name).waitForExist({ timeout, timeoutMsg: `no card for ${name}` });
  await browser
    .waitUntil(
      async () => {
        seen = (await card(name).getText()).replace(/\s+/g, " ");
        return seen.includes(text);
      },
      { timeout, interval: 300 },
    )
    .catch(() => {
      throw new Error(`card ${name}: want "${text}", got "${seen}"`);
    });
}

/** The open app dialog (download confirmation, risky warning). */
export function dialog() {
  return $("dialog.file-dialog");
}

/** Download → confirm in the dialog (the risky warning answers "Download anyway"). */
export async function download(name: string): Promise<void> {
  const button = cardButton(name, "Download");
  await button.waitForClickable({ timeout: 30_000 });
  await button.click();
  const confirm = dialog().$(
    ".//button[normalize-space()='Download' or normalize-space()='Download anyway']",
  );
  await confirm.waitForClickable({ timeout: 10_000 });
  await confirm.click();
}

/** Waits for "✓ Downloaded" with "✓ Matches the offer" on the card of `name`. */
export async function waitDownloaded(name: string, timeout = 180_000): Promise<void> {
  await waitCardText(name, "✓ Downloaded", timeout);
  await waitCardText(name, "✓ Matches the offer", 5_000);
}

/** Whether the Download button of `name` is disabled, and its reason (tooltip text). */
export async function downloadBlock(name: string): Promise<string | null> {
  const button = cardButton(name, "Download");
  await button.waitForExist({ timeout: 30_000 });
  if ((await button.getAttribute("aria-disabled")) !== "true") return null;
  const described = await button.getAttribute("aria-describedby");
  return described ? ((await $(`#${described}`).getProperty("textContent")) as string) : "";
}

/** Waits until the Download of `name` is blocked with `reason` (null: enabled). */
export async function expectDownloadBlock(name: string, reason: string | null): Promise<void> {
  let seen: string | null = "";
  await browser
    .waitUntil(async () => (seen = await downloadBlock(name)) === reason, { timeout: 30_000 })
    .catch(() => {
      throw new Error(
        `Download of ${name}: want ${JSON.stringify(reason)}, got ${JSON.stringify(seen)}`,
      );
    });
  expect(seen).toBe(reason);
}
