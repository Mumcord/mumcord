// Shared steps of the share-sound specs (spec 07 §4.10, AC-7.61 to 7.66 and 7.70, issue #435).
// A is the sharer: it runs with the container's PipeWire, so its test share captures the real
// sound of the test apps (helpers/pwapps.ts). B is the viewer: its file audio backend records
// what it plays, and `heard` reads which test tones arrive there.
//
// What the container cannot show: A's own playback (the test notification, B's voice) goes to
// A's null audio backend, not to PipeWire, so "Mumcord's own sounds stay absent" is checked at
// B but never really offered to the capture. That exclusion by process id is covered by the
// engine's PipeWire tests (crates/engine/src/screen_share/sound/linux.rs).
import { mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { browser, $, $$, expect } from "@wdio/globals";
import type { ChainablePromiseElement } from "webdriverio";
import * as app from "./app.ts";
import * as apps from "./apps.ts";
import { startScreenShare } from "./chat-ui.ts";
import { openAllSettings } from "./settings-ui.ts";
import * as pwapps from "./pwapps.ts";
import { OWN_SOUND_HZ, receivedFrequencies } from "./pwapps.ts";
import { hookEnv, type TestHooks } from "./relayenv.ts";
import { addAndConnect, completeWizard, joinIfReadOnly } from "./voice-ui.ts";
import { RATE, writeWav } from "./voice-audio.ts";

/** The tones of the test apps P1, P2 and P3, as in the spec. */
export const P1_HZ = 440;
export const P2_HZ = 660;
export const P3_HZ = 880;
/** The second stream P1 opens during a window share (AC-7.62). */
export const P1_SECOND_HZ = 550;
/** B's voice: a tone on B's microphone, which must never come back in A's share sound. */
export const B_VOICE_HZ = 1320;
/** Every tone a spec listens for at B. */
export const ALL_TONES = [P1_HZ, P1_SECOND_HZ, P2_HZ, P3_HZ, OWN_SOUND_HZ, B_VOICE_HZ];

export const SHARER = "Alice";
export const VIEWER = "Bob";

let work: string | undefined;

/** B's recorded output (the file backend's out.wav). */
export function viewerOutput(): string {
  if (!work) throw new Error("the viewer has not started");
  return join(work, "b-out.wav");
}

/** Removes the viewer's sound files; call from `after`. */
export function cleanUpSound(): void {
  if (work) rmSync(work, { recursive: true, force: true });
  work = undefined;
}

/**
 * A joins the voice channel of the local test server as the sharer, with the given test hooks.
 * Its log runs at debug level from the start, for the privacy check of AC-7.70.
 */
export async function startSharer(
  hooks: TestHooks,
  port: number,
  name = SHARER,
): Promise<void> {
  await app.restart({ freshProfile: true, env: hookEnv(hooks) });
  await useDebugLog();
  await app.restart();
  await completeWizard(name);
  await allowDirect();
  await addAndConnect(`localhost:${String(port)}`);
  await joinIfReadOnly();
  // The share entry point and the sharer bar need a wide enough window.
  await browser.setWindowSize(1400, 900);
}

/** B joins the same channel as the viewer; its microphone plays a steady tone ("B talks"). */
export async function startViewer(port: number, name = VIEWER): Promise<void> {
  work ??= mkdtempSync(join(tmpdir(), "mumcord-share-sound-"));
  const voice = join(work, "b-in.wav");
  const tone = new Float32Array(RATE * 120);
  for (let i = 0; i < tone.length; i++)
    tone[i] = 0.3 * Math.sin((2 * Math.PI * B_VOICE_HZ * i) / RATE);
  writeWav(voice, tone);
  await apps.startB({ audio: `file:${voice}:${viewerOutput()}` });
  await apps.onB(async () => {
    await completeWizard(name);
    await allowDirect();
    await addAndConnect(`localhost:${String(port)}`);
    await joinIfReadOnly();
    await browser.setWindowSize(1400, 900);
  });
}

/** A starts its share through the start dialog, leaving the Sound switch as it is. */
export async function share(): Promise<void> {
  await startScreenShare();
}

/** A stops its share. */
export async function stopShare(): Promise<void> {
  await $(".sharer-bar button[aria-label='Stop sharing']").click();
  await $("//div[contains(@class,'sharer-bar')]").waitForExist({ reverse: true, timeout: 20_000 });
}

/** B starts watching A's share from its banner in the channel and waits for the picture. */
export async function watch(): Promise<void> {
  await apps.onB(async () => {
    const button = $(
      `//div[contains(@class,'activity-banner')][contains(.,'${SHARER} is sharing their screen')]//button[normalize-space()='Watch']`,
    );
    await button.waitForClickable({ timeout: 30_000, timeoutMsg: "B never saw A's share" });
    await button.click();
    const connecting = $(`//*[contains(text(),"Connecting to ${SHARER}'s screen")]`);
    await browser.waitUntil(async () => !(await connecting.isExisting()), {
      timeout: 45_000,
      timeoutMsg: "B's view of the share never connected",
    });
  });
}

/** The test tones B played in the last `seconds`. */
export function heard(seconds = 1): number[] {
  return receivedFrequencies(viewerOutput(), ALL_TONES, seconds).sort((a, b) => a - b);
}

/** Waits until B plays exactly `tones` (of `ALL_TONES`), then checks it stays so for `holdMs`. */
export async function expectHeard(tones: readonly number[], holdMs = 3000): Promise<void> {
  const want = [...tones].sort((a, b) => a - b);
  let got: number[] = [];
  await browser
    .waitUntil(
      () => {
        got = heard();
        return Promise.resolve(JSON.stringify(got) === JSON.stringify(want));
      },
      { timeout: 30_000, interval: 250 },
    )
    .catch(() => {
      throw new Error(`B plays ${JSON.stringify(got)} Hz, want ${JSON.stringify(want)} Hz`);
    });
  const until = Date.now() + holdMs;
  while (Date.now() < until) {
    await browser.pause(500);
    expect(heard()).toEqual(want);
  }
}

/**
 * Milliseconds from now until B plays `hz` (listening to the last 0.3 s), or throws after
 * `timeoutMs`.
 */
export async function msUntilHeard(
  hz: number,
  present: boolean,
  timeoutMs = 10_000,
): Promise<number> {
  const started = Date.now();
  await browser.waitUntil(
    () => Promise.resolve(receivedFrequencies(viewerOutput(), [hz], 0.3).includes(hz) === present),
    {
      timeout: timeoutMs,
      interval: 50,
      timeoutMsg: `${String(hz)} Hz never became ${present ? "audible" : "silent"} at B`,
    },
  );
  return Date.now() - started;
}

/** A's own share as the engine reports it. */
export interface OwnShare {
  session: number;
  sound: boolean;
  soundMode: "desktop" | "app" | "none";
  soundApp: string | null;
  soundReason: string | null;
  soundChoice: { id: string; name: string }[] | null;
}

/** The shares the current client knows of (`screen_share_list` of its voice server). */
export async function shares(): Promise<OwnShare[]> {
  const serverId = await app.invoke<number | null>("voice_server_get");
  if (serverId === null) throw new Error("no voice server");
  return app.invoke<OwnShare[]>("screen_share_list", { serverId });
}

/**
 * The sound line in A's sharer bar ("Sound: desktop", "Sound: {app}" or "No sound"). The bar
 * exists in a wide and a narrow layout, so this takes the one on screen.
 */
export async function soundLine(): Promise<string | null> {
  const badges = await $$(
    "//div[contains(@class,'sharer-bar')]//span[contains(@class,'share-badge')][starts-with(normalize-space(),'Sound:') or normalize-space()='No sound']",
  );
  for (const badge of badges) if (await badge.isDisplayed()) return (await badge.getText()).trim();
  return null;
}

/** Waits until A's sharer bar says `text` on its sound line. */
export async function expectSoundLine(text: string): Promise<void> {
  let seen: string | null = null;
  await browser
    .waitUntil(
      async () => {
        seen = await soundLine();
        return seen === text;
      },
      { timeout: 20_000 },
    )
    .catch(() => {
      throw new Error(`the sound line says ${JSON.stringify(seen)}, want "${text}"`);
    });
}

/** Switches A's log to debug level, which applies from the next start (AC-7.70). */
export async function useDebugLog(): Promise<void> {
  const settings = await app.invoke<Record<string, unknown>>("settings_get");
  await app.invoke("settings_update", { settings: { ...settings, logLevel: "debug" } });
}

/**
 * Allows direct connections on the current client, the path of a share without a relay.
 * Without that answer the first watch asks both people first; that question is not what these
 * specs are about.
 */
async function allowDirect(): Promise<void> {
  const settings = await app.invoke<Record<string, unknown>>("settings_get");
  await app.invoke("settings_update", { settings: { ...settings, allowDirectConnections: "yes" } });
}

/** A's client log files of the current profile, joined. */
export function sharerLog(): string {
  const dir = join(app.profile("a"), "data", "app.mumcord.client", "logs");
  return readdirSync(dir)
    .map((f) => readFileSync(join(dir, f), "utf8"))
    .join("\n");
}

/**
 * AC-7.70: the log names no test app, binary or process id. A process id counts only next to
 * "pid", so timestamps and ports that happen to contain the same digits do not count.
 */
export function expectLogPrivate(
  apps: readonly { name: string; binary: string; pids: number[] }[],
): void {
  const log = sharerLog();
  expect(log).not.toBe("");
  for (const a of apps) {
    expect(log).not.toContain(a.name);
    expect(log).not.toContain(a.binary);
    for (const pid of a.pids)
      expect(log).not.toMatch(new RegExp(`pid\\W{0,3}${String(pid)}\\b`, "i"));
  }
  expect(log).not.toMatch(/testapp-/);
}

/** What the test apps of a spec are known by; none of it may reach the log (AC-7.70). */
export class PrivateNames {
  private readonly entries: { app: pwapps.TestApp; pids: Set<number> }[] = [];

  /** Remembers `app` with the process ids it has now. */
  add(app: pwapps.TestApp): pwapps.TestApp {
    this.entries.push({ app, pids: new Set(app.pids()) });
    return app;
  }

  /** Adds the process ids the apps have now (players restart, apps open more streams). */
  refresh(): void {
    for (const e of this.entries) for (const pid of e.app.pids()) e.pids.add(pid);
  }

  all(): { name: string; binary: string; pids: number[] }[] {
    this.refresh();
    return this.entries.map((e) => ({ name: e.app.name, binary: e.app.binary, pids: [...e.pids] }));
  }
}

/** A opens the share start dialog from the Apps menu, without confirming it. */
export async function openShareDialog(): Promise<void> {
  await $("button.chat-apps__button").click();
  await $(
    "//div[contains(@class,'chat-apps__menu')]//*[@role='menuitem'][starts-with(normalize-space(),'Share your screen')]",
  ).click();
  await $("//dialog//button[normalize-space()='Choose what to share']").waitForClickable({
    timeout: 20_000,
    timeoutMsg: "the share start dialog never opened",
  });
}

/** Confirms the open start dialog and waits until the share runs. */
export async function confirmShareDialog(): Promise<void> {
  await $("//dialog//button[normalize-space()='Choose what to share']").click();
  await $("//div[contains(@class,'sharer-bar')]").waitForExist({
    timeout: 30_000,
    timeoutMsg: "the screen share never started",
  });
}

/** The Sound switch of the open start dialog. */
export function dialogSoundSwitch() {
  return $("//dialog//input[@role='switch'][@id=//dialog//label[normalize-space()='Sound']/@for]");
}

/** Opens Settings → Screen sharing. */
export async function openScreenSharingSettings(): Promise<void> {
  await openAllSettings();
  // The tab may carry a "New" pill after its name.
  const tab = $("//button[@role='tab'][starts-with(normalize-space(.),'Screen sharing')]");
  await tab.waitForClickable({ timeout: 10_000 });
  await tab.click();
  await $(
    "//h2[contains(@class,'settings-page__title')][normalize-space(.)='Screen sharing']",
  ).waitForDisplayed({ timeout: 10_000 });
}

/** The row of the Sound switch on the open Screen sharing settings page (the one on screen). */
export async function soundSettingRow(): Promise<ChainablePromiseElement> {
  const xpath = "//div[contains(@class,'settings-switch')][.//label[normalize-space()='Sound']]";
  await $(`(${xpath})[1]`).waitForExist({ timeout: 10_000 });
  const rows = await $$(xpath);
  for (let i = 0; i < rows.length; i++) {
    if (await rows[i].isDisplayed()) return $(`(${xpath})[${String(i + 1)}]`);
  }
  throw new Error("no Sound switch on screen");
}

/** The Sound switch on the open Screen sharing settings page. */
export async function soundSetting(): Promise<ChainablePromiseElement> {
  return (await soundSettingRow()).$("input[role='switch']");
}
