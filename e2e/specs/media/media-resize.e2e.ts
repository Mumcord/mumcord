// Spec 07 §4.9 (issue #386, AC-7.44–7.56): the horizontal handle between the media area and the
// chat. A screen share cannot be started in the container (no real screen to pick, see
// share-bar.e2e.ts), so the big view here is a watch party this client starts itself; the screen
// share uses the same media area and handle, and its Fit / Fill / 1:1 rules at a chosen height are
// covered by the vitest suite (`MediaArea.test.tsx`, `mediaHeight.test.ts`). YouTube is not
// reachable from the container, so the party shows its "can't play" card; the frame and its size
// are what this spec checks. Screen-reader output (AC-7.52) is checked here only through the ARIA
// attributes; the camera room (AC-7.57) does not exist yet.
//
// Run: npm run e2e -- --spec e2e/specs/media/media-resize.e2e.ts
import { browser, $, expect } from "@wdio/globals";
import { app, testenv } from "../../helpers/index.ts";
import { ports } from "../../helpers/testenv.ts";
import {
  addFirstServer,
  finishWizard,
  joinChannel,
  reconnectSaved,
} from "../../helpers/chat-ui.ts";

const ME = "ResizeQA";
const VIDEO = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";

const handle = () => $("[role='separator'][aria-label='Resize media area']");
const frame = () => $("#media-area .media-frame");
const picture = () => $("#media-area .media-frame__picture");

async function value(): Promise<number> {
  return Number(await handle().getAttribute("aria-valuenow"));
}

async function pictureHeight(): Promise<number> {
  return (await picture().getSize()).height;
}

async function stored(): Promise<string | null> {
  return browser.execute(() => localStorage.getItem("mumcord.mediaHeight"));
}

/** Rem in pixels and the room the chat keeps below the media area, in rem. */
async function chatRoomRem(): Promise<number> {
  return browser.execute(() => {
    const rem = parseFloat(getComputedStyle(document.documentElement).fontSize);
    const chat = document.querySelector(".chat");
    const handleEl = document.querySelector(".media-area__handle");
    if (chat === null || handleEl === null) return 0;
    return (chat.getBoundingClientRect().bottom - handleEl.getBoundingClientRect().bottom) / rem;
  });
}

/** Starts a watch party from the Apps menu; its big view opens in the chat column. */
async function startParty(): Promise<void> {
  const apps = $("button[aria-label='Apps']");
  await apps.waitForClickable({ timeout: 20_000 });
  await apps.click();
  const watch = $("//button[@role='menuitem'][starts-with(., 'Watch together')]");
  await watch.waitForClickable();
  await watch.click();
  const link = $("//input[@placeholder='Paste a YouTube link']");
  await link.waitForDisplayed();
  await link.setValue(VIDEO);
  await $("//button[normalize-space()='Start']").click();
  await frame().waitForDisplayed({ timeout: 20_000, timeoutMsg: "the party's big view did not open" });
  await handle().waitForDisplayed({ timeout: 10_000, timeoutMsg: "no resize handle" });
}

/** Drags the handle by `dy` pixels with the mouse, in small steps, and releases it. */
async function drag(dy: number, release = true): Promise<void> {
  const steps = 8;
  let pointer = browser
    .action("pointer", { parameters: { pointerType: "mouse" } })
    .move({ origin: await handle() })
    .down({ button: 0 });
  for (let i = 1; i <= steps; i++) {
    pointer = pointer.move({ origin: "pointer", x: 0, y: Math.round(dy / steps), duration: 30 });
  }
  if (release) pointer = pointer.up({ button: 0 });
  await pointer.perform(!release);
}

async function waitValue(check: (v: number) => boolean, what: string): Promise<void> {
  await browser.waitUntil(async () => check(await value()), { timeout: 5_000, timeoutMsg: what });
}

describe("resizable media area (#386)", () => {
  before(async () => {
    await testenv.up("v15");
    await app.restart({ freshProfile: true });
    await finishWizard(ME);
    await addFirstServer(ports.v15);
    await joinChannel("Lobby");
    await browser.setWindowSize(1400, 900);
    await startParty();
  });

  it("AC-7.44: shows the handle with today's size (Auto)", async () => {
    const separator = handle();
    await expect(separator).toHaveAttribute("aria-orientation", "horizontal");
    await expect(separator).toHaveAttribute("aria-controls", "media-area");
    await expect(separator).toHaveAttribute("tabindex", "0");
    expect(await stored()).toBeNull();
    expect(await $("#media-area").getAttribute("data-sized")).toBeNull();
    // Auto: 16:9 and at most 58 % of the window height.
    const size = await picture().getSize();
    expect(Math.abs(size.width / size.height - 16 / 9)).toBeLessThan(0.02);
    const inner = await browser.execute(() => window.innerHeight);
    expect(size.height).toBeLessThanOrEqual(inner * 0.58 + 1);
  });

  it("AC-7.45: the picture follows a drag and the chat gets the room", async () => {
    const before = await pictureHeight();
    await drag(-120);
    const smaller = await pictureHeight();
    expect(smaller).toBeLessThan(before - 60);
    expect(await stored()).not.toBeNull();
    // Nothing got selected while dragging.
    expect(await browser.execute(() => String(window.getSelection() ?? ""))).toBe("");
    await drag(200);
    expect(await pictureHeight()).toBeGreaterThan(smaller + 100);
  });

  it("AC-7.48: stops at the limits and the chat keeps 13 rem", async () => {
    await drag(2000);
    const max = await value();
    expect(max).toBeLessThanOrEqual(70);
    await expect(handle()).toHaveAttribute("aria-valuemax", String(max));
    expect(await chatRoomRem()).toBeGreaterThanOrEqual(12.9);
  });

  it("AC-7.51: keyboard steps, snap jumps and limits, saved on each key", async () => {
    await handle().click();
    await browser.keys("Home");
    const min = await value();
    expect(min).toBeGreaterThanOrEqual(20);
    await expect(handle()).toHaveAttribute("aria-valuemin", String(min));
    const minStored = await stored();
    await browser.keys("ArrowDown");
    await waitValue((v) => v > min, "ArrowDown did not grow the picture");
    expect(await stored()).not.toBe(minStored);
    await browser.keys("PageDown");
    await waitValue((v) => v === 33 || v === 50, "PageDown did not jump to a snap point");
    await browser.keys("End");
    const end = await value();
    await browser.keys("PageUp");
    await waitValue((v) => v < end, "PageUp did not jump down");
    // AC-7.52 as far as automation goes: the spoken value text.
    await expect(handle()).toHaveAttribute(
      "aria-valuetext",
      `${String(await value())}% of the window height`,
    );
  });

  it("AC-7.51: Tab reaches the handle right after the media controls", async () => {
    const lastControl = $("#media-area .media-frame__controls button:last-of-type");
    await lastControl.click();
    await browser.keys("Tab");
    const focused = await browser.execute(() => document.activeElement?.getAttribute("aria-label"));
    expect(focused).toBe("Resize media area");
  });

  it("AC-7.50: Enter and a double-click go back to Auto", async () => {
    await handle().click();
    await browser.keys("Home");
    await browser.keys("Enter");
    await browser.waitUntil(async () => (await stored()) === '"auto"', {
      timeout: 5_000,
      timeoutMsg: "Enter did not reset",
    });
    await browser.keys("Home");
    await handle().doubleClick();
    await browser.waitUntil(async () => (await stored()) === '"auto"', {
      timeout: 5_000,
      timeoutMsg: "double-click did not reset",
    });
    await browser.waitUntil(async () => (await $("#media-area").getAttribute("data-sized")) === null);
  });

  it("AC-7.49: dragging below the minimum minimises, Open restores the height", async () => {
    await handle().click();
    await browser.keys("PageDown");
    await browser.keys("PageDown");
    const chosen = await value();
    const kept = await stored();
    await drag(-1500, false);
    await $("//*[contains(@class,'media-area__collapse')][contains(., 'Release to minimise to the mini bar')]").waitForDisplayed({ timeout: 5_000 });
    await browser.action("pointer").up({ button: 0 }).perform();
    await frame().waitForExist({ reverse: true, timeout: 5_000 });
    const open = $("//button[normalize-space()='Open']");
    await open.waitForClickable({ timeout: 5_000 });
    expect(await stored()).toBe(kept);
    await open.click();
    await handle().waitForDisplayed({ timeout: 10_000 });
    await waitValue((v) => v === chosen, "Open did not restore the chosen height");
  });

  it("AC-7.54: a small window shrinks the picture, keeps the chat and the stored value", async () => {
    await handle().click();
    await browser.keys("End");
    const kept = await stored();
    await browser.setWindowSize(1400, 640);
    await browser.waitUntil(async () => (await chatRoomRem()) >= 12.9, { timeout: 5_000 });
    expect(await stored()).toBe(kept);
    await browser.setWindowSize(1400, 420);
    await handle().waitForExist({ reverse: true, timeout: 5_000 });
    await browser.setWindowSize(1400, 900);
    await handle().waitForDisplayed({ timeout: 5_000 });
    expect(await stored()).toBe(kept);
  });

  it("AC-7.55: full screen ignores the height and has no handle", async () => {
    const chosen = await value();
    await frame().click();
    await browser.keys("f");
    await browser.waitUntil(async () => browser.execute(() => document.fullscreenElement !== null), {
      timeout: 5_000,
      timeoutMsg: "no full screen",
    });
    expect(await handle().isExisting()).toBe(false);
    await browser.keys("Escape");
    await handle().waitForDisplayed({ timeout: 5_000 });
    expect(await value()).toBe(chosen);
  });

  it("AC-7.53: the height survives closing the view and a restart", async () => {
    await handle().click();
    await browser.keys("Home");
    await browser.keys("PageDown");
    const chosen = await value();
    const kept = await stored();
    await app.restart();
    await browser.setWindowSize(1400, 900);
    await reconnectSaved();
    await joinChannel("Lobby");
    expect(await stored()).toBe(kept);
    await startParty();
    await waitValue((v) => v === chosen, "the height was not restored after a restart");
  });
});
