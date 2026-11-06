// Spec 08, the camera strip next to a screen share or a watch party: AC-8.14 (mockups 08 and
// 09). The share or party is the main picture; the switch "Cameras" moves the cameras beside,
// above or hides them; Hidden unbinds the viewer from every camera broadcast; ending the share
// or leaving the party returns the camera grid.
//
// The relay has no API that lists the viewers of a broadcast, so "unbinds" is read from the
// sharers' side: A's and C's camera views stop listing B. YouTube is not reachable from the
// container, so the party shows its "can't play" card; the layout around it is what counts here.
//
// Skips itself without a mumpeer checkout (see relay-uses.e2e.ts).
// Run: npm run e2e -- --spec e2e/specs/camera/layout.e2e.ts
import { $, browser, expect } from "@wdio/globals";
import { apps, relayenv } from "../../helpers/index.ts";
import { startScreenShare } from "../../helpers/chat-ui.ts";
import {
  NAMES,
  cameraView,
  on,
  sessionOf,
  setRoomOpen,
  startCamClient,
  turnCameraOn,
  updateSettings,
  waitTile,
} from "./flows.ts";

const VIDEO = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";

/** The combined view: the share or party with the camera strip. */
const combined = () => $("section.cam-combined");

/** A radio of the "Cameras" switch in the combined view's header. */
const position = (label: string) =>
  $(`//section[contains(@class,'cam-combined')]//*[@role='radiogroup'][@aria-label='Cameras']//*[@role='radio'][normalize-space()='${label}']`);

/**
 * Makes `id` the main item of the media area through its stage tab. A new item never takes the
 * stage from the open camera room; it waits as a tab with a dot (spec 09 FR-9.44).
 */
async function pickStage(id: "share" | "watch"): Promise<void> {
  const tab = $(`#stage-tab-${id}`);
  await tab.waitForClickable({ timeout: 20_000, timeoutMsg: `no stage tab for ${id}` });
  await tab.click();
}

/** Strip tiles in the combined view. */
function stripTiles(): Promise<number> {
  return browser.execute(() => document.querySelectorAll("section.cam-combined .cam-strip [data-tile]").length);
}

/** Waits until the camera view of the current client lists `viewer` (or not). */
async function waitViewer(viewer: number, present: boolean): Promise<void> {
  await browser.waitUntil(async () => ((await cameraView())?.viewers ?? []).includes(viewer) === present, {
    timeout: 20_000,
    interval: 250,
    timeoutMsg: `viewer ${String(viewer)} ${present ? "never joined" : "stayed bound"}`,
  });
}

/** On B: walks the switch through Beside, Above and Hidden and back, checking the strip. */
async function walkPositions(bob: number): Promise<void> {
  await on("b", async () => {
    await combined().waitForDisplayed({ timeout: 30_000 });
    // The cameras sit beside the main picture by default (mockup 08).
    expect(await combined().getAttribute("data-position")).toBe("beside");
    await browser.waitUntil(async () => (await stripTiles()) === 2, {
      timeout: 20_000,
      timeoutMsg: "the strip does not show A's and C's cameras",
    });
    await position("Above").click();
    await browser.waitUntil(async () => (await combined().getAttribute("data-position")) === "above", {
      timeoutMsg: "the strip did not move above",
    });
    expect(await stripTiles()).toBe(2);
    await position("Hidden").click();
    await browser.waitUntil(async () => (await combined().getAttribute("data-position")) === "hidden", {
      timeoutMsg: "the strip did not hide",
    });
    expect(await stripTiles()).toBe(0);
  });
  // Hidden unbinds B from both camera broadcasts.
  await waitViewer(bob, false);
  await on("c", () => waitViewer(bob, false));

  await on("b", async () => {
    await position("Cameras beside").click();
    await browser.waitUntil(async () => (await stripTiles()) === 2, {
      timeout: 20_000,
      timeoutMsg: "the strip did not come back",
    });
  });
  await waitViewer(bob, true);
  await on("c", () => waitViewer(bob, true));
}

/** On B: the combined view is gone and the room shows its grid again. */
async function expectGrid(): Promise<void> {
  await on("b", async () => {
    await combined().waitForExist({ reverse: true, timeout: 20_000 });
    await $(".cam-room .cam-grid").waitForDisplayed({ timeout: 10_000 });
    await waitTile(NAMES.a);
    await waitTile(NAMES.c);
  });
}

(relayenv.relayAvailable() ? describe : describe.skip)("camera strip with a screen share or a watch party (AC-8.14)", function () {
  this.timeout(900_000);
  this.bail(true);

  let bob = 0;

  before(async () => {
    const { port } = await relayenv.startRelay("all");
    await startCamClient("a", port);
    await startCamClient("b", port);
    await startCamClient("c", port, { env: relayenv.hookEnv({ sharePick: "screen" }) });
    await on("b", async () => {
      await updateSettings({ cameraShow: "always" });
      await setRoomOpen(true);
    });
    await turnCameraOn();
    await on("c", turnCameraOn);
    await on("b", async () => {
      await waitTile(NAMES.a);
      await waitTile(NAMES.c);
    });
    bob = await sessionOf(NAMES.b);
  });

  after(async () => {
    await apps.stopB();
    await apps.stopC();
    await relayenv.stopRelay();
  });

  it("AC-8.14: C's screen share is the main picture; the switch moves the cameras; Hidden unbinds", async () => {
    await on("c", startScreenShare);
    await on("b", async () => {
      const watch = $(
        `//div[contains(@class,'activity-banner')][contains(.,'${NAMES.c} is sharing their screen')]//button[normalize-space()='Watch']`,
      );
      await watch.waitForClickable({ timeout: 30_000, timeoutMsg: "B never saw C's share" });
      await watch.click();
      await pickStage("share");
      await combined().waitForDisplayed({ timeout: 30_000 });
      expect(await combined().getAttribute("aria-label")).toContain("with the camera room");
    });
    await walkPositions(bob);
  });

  it("AC-8.14: ending the share returns the grid", async () => {
    await on("c", async () => {
      await $(".sharer-bar button[aria-label='Stop sharing']").click();
      await $("//div[contains(@class,'sharer-bar')]").waitForExist({ reverse: true, timeout: 20_000 });
    });
    await expectGrid();
  });

  it("AC-8.14: the same with a watch party (mockup 09)", async () => {
    await on("b", async () => {
      const appsMenu = $("button[aria-label='Apps']");
      await appsMenu.waitForClickable({ timeout: 20_000 });
      await appsMenu.click();
      await $("//button[@role='menuitem'][starts-with(., 'Watch together')]").click();
      const link = $("//input[@placeholder='Paste a YouTube link']");
      await link.waitForDisplayed();
      await link.setValue(VIDEO);
      await $("//button[normalize-space()='Start']").click();
      await pickStage("watch");
      await combined().waitForDisplayed({ timeout: 30_000 });
      expect(await combined().getAttribute("aria-label")).toBe("Watch party with the camera room");
    });
    await walkPositions(bob);
    await on("b", async () => {
      const leave = $("//button[normalize-space()='Leave party']");
      await leave.waitForClickable({ timeout: 10_000 });
      await leave.click();
    });
    await expectGrid();
  });
});
