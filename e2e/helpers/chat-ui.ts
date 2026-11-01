// UI and bot helpers for the chat/follow specs (e2e/specs/chat, AC-1.13–AC-1.18).
import { chmodSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { browser, expect, $, $$ } from "@wdio/globals";
import { botInBackground, type BackgroundBot, type BotOptions } from "./bot.ts";
import { artifactsDir, repoRoot } from "./paths.ts";
import { run } from "./run.ts";
import { runFirstStartWizard, type WizardPreset } from "./wizard-util.ts";
import { doubleClickRow, showAllPeople } from "./voice-ui.ts";

export const USER = "ChatQA";

/** Runs the first-start wizard: new identity, default devices and voice activation, `preset`. */
export async function finishWizard(name = USER, preset?: WizardPreset): Promise<void> {
  await runFirstStartWizard(name, preset);
}

export async function clickButton(text: string): Promise<void> {
  const button = $(`//button[normalize-space(.)='${text}']`);
  await button.waitForClickable();
  await button.click();
}

/** Adds a server via the empty state and connects, trusting its certificate. */
export async function addFirstServer(port: number): Promise<void> {
  const input = $("//label[.='Server address or mumble:// link']/following::input[1]");
  await input.waitForDisplayed({ timeout: 30_000 });
  await input.setValue(`localhost:${String(port)}`);
  await clickButton("Add and connect");
  await trustIfAsked();
  await waitConnected();
}

export async function trustIfAsked(): Promise<void> {
  const trust = $("//button[.='Trust and connect']");
  if (await trust.waitForDisplayed({ timeout: 15_000 }).catch(() => false)) await trust.click();
}

export async function waitConnected(): Promise<void> {
  await $("//button[contains(@aria-label,'(connected)')]").waitForExist({ timeout: 30_000 });
  await channelName("Lobby").waitForDisplayed({ timeout: 15_000 });
  await showAllPeople();
}

/** Clicks the saved server in the server bar (after a restart nothing auto-connects). */
export async function reconnectSaved(): Promise<void> {
  const server = $("//ul[@aria-label='Saved servers']//button[@aria-label]");
  await server.waitForClickable({ timeout: 30_000 });
  await server.click();
  await trustIfAsked();
  await waitConnected();
}

export function channelName(name: string) {
  return $(`//span[contains(@class,'tree-channel__name') and normalize-space(.)='${name}']`);
}

function channelRow(name: string): string {
  return `//span[contains(@class,'tree-channel__name') and normalize-space(.)='${name}']/ancestor::div[contains(@class,'tree-channel__row')][1]`;
}

/** Opens the private messages overview (#173) from the server bar. */
export async function openMessagesOverview(): Promise<void> {
  const button = $("[data-testid='pm-button']");
  await button.waitForClickable({ timeout: 15_000 });
  await button.click();
  await $("[data-testid='pm-overview']").waitForDisplayed();
}

/** Back from the private messages overview to the channel tree of the first server. */
export async function closeMessagesOverview(): Promise<void> {
  await $(
    "(//ul[contains(@class,'server-bar__list')]//button[contains(@class,'server-bar__server')])[1]",
  ).click();
  await $("[data-testid='pm-overview']").waitForExist({ reverse: true });
}

/** The conversation row of `name` in the private messages overview. */
export function pmRow(name: string) {
  return $(`//button[@data-testid='pm-row'][starts-with(@aria-label,'${name},')]`);
}

/** Shows the channel's chat (single click). */
export async function selectChannel(name: string): Promise<void> {
  const header = $(
    `//h2[contains(@class,'chat-header__title') and contains(normalize-space(.),'${name}')]`,
  );
  // The tree reflows when people/listeners appear; a click can land on a person row and open
  // a profile card. Close it (Escape) and retry.
  for (let attempt = 0; attempt < 3; attempt++) {
    await sleep(300);
    await channelName(name).click();
    if (await header.waitForDisplayed({ timeout: 3_000 }).catch(() => false)) return;
    await browser.keys("Escape");
  }
  await header.waitForDisplayed();
}

/** Joins a channel (double-click) and waits until "(you)" is listed under it. */
export async function joinChannel(name: string): Promise<void> {
  await showAllPeople();
  await doubleClickRow(channelName(name));
  await browser.waitUntil(
    async () =>
      (await $$(
        `${channelRow(name)}/ancestor::div[@role='treeitem'][1]/following-sibling::div[@role='group'][1]//span[contains(@class,'tree-person__you')]`,
      ).length) > 0,
    { timeoutMsg: `did not join ${name}` },
  );
}

/** The unread badge count of a channel row ("" when there is none). The visible number is drawn
 * from the badge's data-count (CSS), so read that attribute. */
export async function unreadBadge(name: string): Promise<string> {
  const badge = $(`${channelRow(name)}//span[contains(@class,'unread-badge')]`);
  return (await badge.isExisting()) ? ((await badge.getAttribute("data-count")) ?? "") : "";
}

/** Inner HTML of every message body in the open chat. */
export async function messageBodies(): Promise<string[]> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll(".chat-list__items > li")).map((li) => li.innerHTML),
  );
}

export async function messageTexts(): Promise<string[]> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll(".chat-list__items > li")).map(
      (li) => (li as HTMLElement).innerText,
    ),
  );
}

export async function waitMessage(contains: string, timeout = 15_000): Promise<void> {
  await browser.waitUntil(async () => (await messageTexts()).some((t) => t.includes(contains)), {
    timeout,
    timeoutMsg: `no message containing ${JSON.stringify(contains)}`,
  });
}

export function composer() {
  return $("textarea[aria-label^='Message']");
}

/** Types into the composer and presses Enter. */
export async function sendText(text: string): Promise<void> {
  const box = composer();
  await box.waitForEnabled();
  await box.setValue(text);
  await browser.keys("Enter");
}

/**
 * Pastes an image into the composer: a canvas-rendered PNG wrapped in a File and delivered as a
 * real `paste` event with clipboardData (WebDriver cannot fill the OS clipboard with an image).
 * `noise` makes it incompressible so the size limit kicks in.
 */
export async function pasteImage(width: number, height: number, noise: boolean): Promise<void> {
  await browser.execute(
    async (w: number, h: number, n: boolean) => {
      const canvas = document.createElement("canvas");
      canvas.width = w;
      canvas.height = h;
      const ctx = canvas.getContext("2d");
      if (!ctx) throw new Error("no canvas");
      if (n) {
        const data = ctx.createImageData(w, h);
        for (let i = 0; i < data.data.length; i++) data.data[i] = (Math.random() * 256) | 0;
        ctx.putImageData(data, 0, 0);
      } else {
        ctx.fillStyle = "#e33";
        ctx.fillRect(0, 0, w, h);
      }
      const blob = await new Promise<Blob>((resolve, reject) => {
        canvas.toBlob((b) => {
          if (b) resolve(b);
          else reject(new Error("toBlob failed"));
        }, "image/png");
      });
      const file = new File([blob], "pasted.png", { type: "image/png" });
      const dt = new DataTransfer();
      dt.items.add(file);
      const box = document.querySelector("textarea[aria-label^='Message']");
      if (!box) throw new Error("no composer");
      (box as HTMLElement).focus();
      const event = new ClipboardEvent("paste", {
        bubbles: true,
        cancelable: true,
        clipboardData: dt,
      });
      if (!event.clipboardData || event.clipboardData.files.length === 0) {
        Object.defineProperty(event, "clipboardData", { value: dt });
      }
      box.dispatchEvent(event);
    },
    width,
    height,
    noise,
  );
}

/** A `mumbot script` session: `run(line)` sends a line and waits for its "ok" echo. */
export class ScriptBot {
  private readonly bot: BackgroundBot;
  private out = "";
  private readonly ready: Promise<void>;

  constructor(opts: BotOptions) {
    this.bot = botInBackground(["script"], { timeout: 20, ...opts });
    this.bot.process.stdout?.on("data", (d: Buffer) => (this.out += d.toString()));
    this.bot.done.catch(() => undefined);
    // `connect` goes through the same echo, so it doubles as a login barrier.
    this.ready = this.run("sleep 0").then(() => undefined);
  }

  async connected(): Promise<void> {
    await this.ready;
  }

  /** Sends one script line; resolves with the output it produced (before its "ok" line). */
  async run(line: string, timeoutMs = 30_000): Promise<string> {
    const start = this.out.length;
    this.bot.send(line);
    const marker = `ok ${line}\n`;
    const deadline = Date.now() + timeoutMs;
    for (;;) {
      const idx = this.out.indexOf(marker, start);
      if (idx >= 0) return this.out.slice(start, idx);
      if (this.bot.process.exitCode !== null) {
        const err = await this.bot.done.then(() => "exited 0").catch((e: unknown) => String(e));
        throw new Error(`bot exited during ${JSON.stringify(line)}: ${err}`);
      }
      if (Date.now() > deadline) throw new Error(`bot line timed out: ${line}`);
      await new Promise((r) => setTimeout(r, 100));
    }
  }

  stop(): void {
    this.bot.stop();
  }
}

/** Directory with fake `xdg-open`/`gio` that log their arguments instead of opening a browser. */
export function installOpenerStub(): { log: string; binDir: string } {
  const dir = join(artifactsDir, "chat-opener-stub");
  const binDir = join(dir, "bin");
  const log = join(dir, "opened.log");
  mkdirSync(binDir, { recursive: true });
  writeFileSync(log, "");
  for (const name of ["xdg-open", "gio", "gnome-open", "kde-open"]) {
    const path = join(binDir, name);
    writeFileSync(path, `#!/bin/sh\necho "${name} $*" >> "${log}"\n`);
    chmodSync(path, 0o755);
  }
  return { log, binDir };
}

/** Writes a silent 48 kHz mono WAV (app microphone for audio specs). */
export function silentWav(name: string, seconds: number): string {
  const samples = 48_000 * seconds;
  const header = Buffer.alloc(44);
  header.write("RIFF", 0);
  header.writeUInt32LE(36 + samples * 2, 4);
  header.write("WAVEfmt ", 8);
  header.writeUInt32LE(16, 16);
  header.writeUInt16LE(1, 20);
  header.writeUInt16LE(1, 22);
  header.writeUInt32LE(48_000, 24);
  header.writeUInt32LE(96_000, 28);
  header.writeUInt16LE(2, 32);
  header.writeUInt16LE(16, 34);
  header.write("data", 36);
  header.writeUInt32LE(samples * 2, 40);
  mkdirSync(artifactsDir, { recursive: true });
  const path = join(artifactsDir, name);
  writeFileSync(path, Buffer.concat([header, Buffer.alloc(samples * 2)]));
  return path;
}

/** The app's output recording (`file:` backend): samples written so far. */
export class OutputWav {
  constructor(readonly path: string) {}

  private dataStart(buf: Buffer): number {
    let pos = 12;
    while (pos + 8 <= buf.length) {
      if (buf.toString("ascii", pos, pos + 4) === "data") return pos + 8;
      pos += 8 + buf.readUInt32LE(pos + 4);
    }
    return 44;
  }

  /** Number of samples currently in the file (from its size; the header lags up to 1 s). */
  length(): number {
    try {
      return Math.max(0, Math.floor((statSync(this.path).size - 44) / 2));
    } catch {
      return 0;
    }
  }

  /** Peak RMS in dBFS over 100 ms windows between two sample positions. */
  peakDb(from: number, to: number): number {
    const buf = readFileSync(this.path);
    const start = this.dataStart(buf);
    const end = Math.min(to, Math.floor((buf.length - start) / 2));
    const win = 4800;
    let peak = 0;
    for (let w = from; w + win <= end; w += win) {
      let sum = 0;
      for (let i = w; i < w + win; i++) {
        const s = buf.readInt16LE(start + i * 2) / 32768;
        sum += s * s;
      }
      peak = Math.max(peak, Math.sqrt(sum / win));
    }
    return peak > 0 ? 20 * Math.log10(peak) : -Infinity;
  }
}

export function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

/**
 * Checks that an engine image is shown through a `blob:` URL (issue #109: no long `data:` URL in
 * `<img src>`) and was actually decoded. Never a web address: the webview loads nothing itself.
 */
export async function expectBlobImage(img: ReturnType<typeof $>): Promise<void> {
  await expect(img).toHaveAttribute("src", expect.stringMatching(/^blob:/));
  await browser.waitUntil(
    async () =>
      browser.execute(
        (el: HTMLElement) => el instanceof HTMLImageElement && el.complete && el.naturalWidth > 0,
        await img.getElement(),
      ),
    { timeout: 10_000, timeoutMsg: "image not decoded" },
  );
}

/**
 * What the app put on the system (X11) clipboard, read like a pasting application does: the
 * text, or `png <bytes>` for an image; "" when the clipboard has nothing of that kind.
 */
export async function readClipboard(kind: "text" | "png"): Promise<string> {
  const script = join(repoRoot, "e2e", "helpers", "x11-clipboard.py");
  const result = await run("python3", ["-I", script, kind]);
  return result.code === 0 ? result.stdout : "";
}
