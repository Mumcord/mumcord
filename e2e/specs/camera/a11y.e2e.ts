// Spec 08, the camera room by keyboard and screen reader and with reduced motion: AC-8.15
// (keyboard only: reach the room, arrow through the tiles, Enter opens the tile menu, P pins,
// H hides, Quality → Data saver; the tile names and the live region) and AC-8.16 (reduce motion
// from the system and from the app's switch: a static speaking frame, no layout animation).
//
// The system setting is GTK's `gtk-enable-animations=0`, which WebKitGTK reports to the page as
// `prefers-reduced-motion: reduce`; B gets it through a settings.ini in `XDG_CONFIG_DIRS`.
// Manual: Windows "Animation effects" off and macOS "Reduce motion" on give the same result;
// a real screen reader (Orca, NVDA, VoiceOver) reads the names checked here.
// "`want` = 720 at the sender" after pinning is read in A's debug panel by hand; here the pinned
// tile is the large one of the Speaker layout.
//
// Skips itself without a mumpeer checkout (see relay-uses.e2e.ts).
// Run: npm run e2e -- --spec e2e/specs/camera/a11y.e2e.ts
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { $, browser, expect } from "@wdio/globals";
import { apps, relayenv } from "../../helpers/index.ts";
import { RATE, writeWav } from "../../helpers/voice-audio.ts";
import {
  NAMES,
  on,
  openCameraSettings,
  setRoomOpen,
  settingsSwitch,
  startCamClient,
  tileOf,
  turnCameraOn,
  updateSettings,
  waitTile,
} from "./flows.ts";

const work = mkdtempSync(join(tmpdir(), "mumcord-camera-a11y-"));

/** A steady tone as A's microphone, so A is "speaking" the whole time. */
function talkingAudio(): string {
  const voice = join(work, "a-in.wav");
  const tone = new Float32Array(RATE * 300);
  for (let i = 0; i < tone.length; i++) tone[i] = 0.3 * Math.sin((2 * Math.PI * 440 * i) / RATE);
  writeWav(voice, tone);
  return `file:${voice}:${join(work, "a-out.wav")}`;
}

/** A GTK settings folder that turns animations off system-wide. */
function noAnimationsConfig(): string {
  const dir = join(work, "xdg");
  mkdirSync(join(dir, "gtk-3.0"), { recursive: true });
  writeFileSync(join(dir, "gtk-3.0", "settings.ini"), "[Settings]\ngtk-enable-animations=0\n");
  return dir;
}

/** The focused element's aria-label (or its text). */
function focusedLabel(): Promise<string> {
  return browser.execute(() => {
    const el = document.activeElement;
    return el?.getAttribute("aria-label") ?? el?.textContent ?? "";
  });
}

/** Tabs forward until a camera tile has the focus (keyboard only, like a user). */
async function tabToRoom(): Promise<void> {
  for (let i = 0; i < 80; i++) {
    const inTile = await browser.execute(() => document.activeElement?.classList.contains("cam-tile") ?? false);
    if (inTile) return;
    await browser.keys("Tab");
  }
  throw new Error("Tab never reached a camera tile");
}

/** Arrows to the tile whose label starts with `name`. */
async function arrowTo(name: string): Promise<void> {
  for (let i = 0; i < 6; i++) {
    if ((await focusedLabel()).startsWith(`${name},`)) return;
    await browser.keys("ArrowRight");
  }
  throw new Error(`the arrow keys never reached ${name}'s tile`);
}

/** From a focused tile, Shift+Tab back to the layout switch and arrow to Speaker. */
async function speakerLayoutByKeyboard(): Promise<void> {
  const inSwitch = () =>
    browser.execute(
      () => (document.activeElement?.closest("[role='radiogroup'][aria-label='Layout']") ?? null) !== null,
    );
  for (let i = 0; i < 20 && !(await inSwitch()); i++) await browser.keys(["Shift", "Tab"]);
  if (!(await inSwitch())) throw new Error("Shift+Tab never reached the layout switch");
  await browser.keys("ArrowRight");
  await $("//span[@role='radiogroup'][@aria-label='Layout']//*[@role='radio'][normalize-space()='Speaker'][@aria-checked='true']").waitForExist({ timeout: 5_000 });
}

/** What the live regions of the window say right now. */
function liveText(): Promise<string> {
  return browser.execute(() =>
    [...document.querySelectorAll("[aria-live]")].map((el) => el.textContent).join(" | "),
  );
}

/** The running CSS animations and transitions in the camera room (tiles and layout). */
function roomAnimations(): Promise<number> {
  return browser.execute(() => {
    const room = document.querySelector(".cam-room");
    return room ? room.getAnimations({ subtree: true }).length : -1;
  });
}

/** The speaking frame of `name`'s tile: its animation and transition, as computed. */
function speakingStyle(name: string): Promise<{ animation: string; transition: string } | null> {
  return browser.execute((who: string) => {
    const tile = [...document.querySelectorAll<HTMLElement>(".cam-tile[data-speaking]")].find((el) =>
      (el.getAttribute("aria-label") ?? "").startsWith(`${who},`),
    );
    if (!tile) return null;
    const styles = [tile, ...tile.querySelectorAll<HTMLElement>("*")].map((el) => getComputedStyle(el));
    return {
      animation: styles.map((s) => s.animationName).filter((n) => n !== "none").join(","),
      transition: styles
        .map((s) => s.transitionDuration)
        .filter((d) => d.split(",").some((part) => parseFloat(part) > 0))
        .join(","),
    };
  }, name);
}

/** Switches the room layout by keyboard and checks that nothing animates. */
async function switchLayoutWithoutAnimation(): Promise<void> {
  const speaker = $("//span[@role='radiogroup'][@aria-label='Layout']//*[@role='radio'][normalize-space()='Speaker']");
  await speaker.click();
  expect(await roomAnimations()).toBe(0);
  await $("//span[@role='radiogroup'][@aria-label='Layout']//*[@role='radio'][normalize-space()='Grid']").click();
  expect(await roomAnimations()).toBe(0);
}

(relayenv.relayAvailable() ? describe : describe.skip)("camera room keyboard, screen reader and reduced motion (AC-8.15, 8.16)", function () {
  this.timeout(900_000);
  this.bail(true);

  before(async () => {
    const { port } = await relayenv.startRelay("all");
    await startCamClient("a", port, { audio: talkingAudio() });
    // B's system asks for reduced motion; the app follows it ("System", the default).
    await startCamClient("b", port, { env: { XDG_CONFIG_DIRS: noAnimationsConfig() } });
    await startCamClient("c", port);
    for (const id of ["b", "c"] as const) {
      await on(id, async () => {
        await updateSettings({ cameraShow: "always" });
        await setRoomOpen(true);
      });
    }
    await turnCameraOn();
    await on("b", () => waitTile(NAMES.a));
  });

  after(async () => {
    await apps.stopB();
    await apps.stopC();
    await relayenv.stopRelay();
  });

  it("AC-8.15: the live region announces a new camera; the tile names say who speaks", async () => {
    await on("c", turnCameraOn);
    await on("b", async () => {
      await browser.waitUntil(async () => (await liveText()).includes(`${NAMES.c} turned on their camera`), {
        timeout: 15_000,
        timeoutMsg: "the live region never announced C's camera",
      });
      await waitTile(NAMES.c);
      const tileA = $(`//div[contains(@class,'cam-tile')][starts-with(@aria-label,'${NAMES.a},')]`);
      await browser.waitUntil(async () => ((await tileA.getAttribute("aria-label")) ?? "").includes("speaking"), {
        timeout: 15_000,
        timeoutMsg: "A's tile name never said speaking",
      });
      expect(await tileA.getAttribute("aria-label")).toMatch(new RegExp(`^${NAMES.a}, camera on.*, speaking`));
    });
  });

  it("AC-8.15: keyboard only: arrows, Enter opens the menu, P pins, H hides, Quality → Data saver", async () => {
    await on("b", async () => {
      await tabToRoom();
      await arrowTo(NAMES.a);
      await browser.keys("Enter");
      const menu = $(`//*[@role='menu'][@aria-label='Options for ${NAMES.a}']`);
      await menu.waitForDisplayed({ timeout: 5_000 });
      for (const item of ["Pin for me", "Show large", `Hide ${NAMES.a}'s video`]) {
        await menu.$(`.//*[@role='menuitem'][starts-with(normalize-space(),"${item}")]`).waitForExist();
      }
      await browser.keys("Escape");
      await menu.waitForExist({ reverse: true });

      // P pins, and the Speaker layout shows the pinned tile large (FR-8.64). C is pinned, so
      // the large tile is the pin and not A, who speaks the whole time.
      await speakerLayoutByKeyboard();
      await tabToRoom();
      await arrowTo(NAMES.c);
      await browser.keys("p");
      await $(`//*[contains(@class,'cam-speaker__large')]//div[contains(@class,'cam-tile')][starts-with(@aria-label,'${NAMES.c},')]`).waitForExist({ timeout: 5_000 });

      // Quality → Data saver on C's tile, through the menu.
      await tabToRoom();
      await arrowTo(NAMES.c);
      await browser.keys("Enter");
      const menuC = $(`//*[@role='menu'][@aria-label='Options for ${NAMES.c}']`);
      await menuC.waitForDisplayed({ timeout: 5_000 });
      for (let i = 0; i < 10; i++) {
        if ((await focusedLabel()).startsWith("Data saver")) break;
        await browser.keys("ArrowDown");
      }
      await browser.keys("Enter");
      await browser.waitUntil(async () => (await tileOf(NAMES.c))?.quality === "dataSaver", {
        timeout: 10_000,
        timeoutMsg: "Data saver did not reach C's tile",
      });

      // H hides A's video.
      await tabToRoom();
      await arrowTo(NAMES.a);
      await browser.keys("h");
      await waitTile(NAMES.a, "hidden");
    });
  });

  it("AC-8.16: with the system's reduce motion, the speaking frame is static and layouts don't animate", async () => {
    await on("b", async () => {
      const alice = await tileOf(NAMES.a);
      if (alice?.hidden) {
        await $(`//div[contains(@class,'cam-tile')][starts-with(@aria-label,'${NAMES.a},')]//button[normalize-space()='Show video']`).click();
        await waitTile(NAMES.a);
      }
      await $(".cam-room[data-reduce-motion]").waitForExist({ timeout: 10_000 });
      await browser.waitUntil(async () => (await speakingStyle(NAMES.a)) !== null, {
        timeout: 15_000,
        timeoutMsg: "A's tile never showed the speaking frame",
      });
      expect(await speakingStyle(NAMES.a)).toEqual({ animation: "", transition: "" });
      await switchLayoutWithoutAnimation();
    });
  });

  it("AC-8.16: the app's switch does the same without the system setting", async () => {
    await on("c", async () => {
      await waitTile(NAMES.a);
      // Without reduce motion the speaking frame moves, so the check below means something.
      await browser.waitUntil(async () => (await speakingStyle(NAMES.a)) !== null, { timeout: 15_000 });
      const moving = await speakingStyle(NAMES.a);
      expect(`${moving?.animation ?? ""}${moving?.transition ?? ""}`).not.toBe("");

      await openCameraSettings();
      const reduce = settingsSwitch("Reduce motion");
      if (!(await reduce.isSelected())) await reduce.click();
      await browser.keys("Escape");
      await $(".cam-room[data-reduce-motion]").waitForExist({ timeout: 10_000 });
      expect(await speakingStyle(NAMES.a)).toEqual({ animation: "", transition: "" });
      await switchLayoutWithoutAnimation();
    });
  });
});
