// Screenshots for the 0.7.0 docs (issue #565), camera part: the README hero, the camera room and
// its menus, the background effects, Settings → Camera, Screen sharing, Hotkeys and the file
// sharing card, the screen-share start dialog, a watched share with the camera strip and the
// viewer controls, a shared file, and the activity log in the members column, each in dark and
// light. Real clients on the mumpeer test environment (profile `all`): A = Alex takes the
// pictures, B = Maya and C = Jonas have cameras on, three bots fill the channel. Every name is
// made up; the cameras are the virtual test camera with its fixture person mask.
//
// A runs on a display of its own at 2x scale, so its pictures come out at 2880×1800 like the
// existing README hero; the others are scaled to 1440×900 afterwards.
//
// Not part of the suite (outside specs/). Run:
//   npm run e2e -- --spec e2e/walkthrough/screenshots-camera.e2e.ts
import type { ChildProcess } from "node:child_process";
import { copyFileSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { $, browser } from "@wdio/globals";
import { apps, relayenv, repoRoot } from "../helpers/index.ts";
import { botInBackground, type BackgroundBot } from "../helpers/bot.ts";
import * as camera from "../helpers/camera.ts";
import { sendText, startScreenShare } from "../helpers/chat-ui.ts";
import * as files from "../helpers/files-ui.ts";
import { openAllSettings } from "../helpers/settings-ui.ts";
import {
  on,
  setRoomOpen,
  startCamClient,
  updateSettings,
  waitTile,
} from "../specs/camera/flows.ts";
import {
  HIDPI_ENV,
  both,
  closeOverlays,
  moduleBot,
  onHidpiDisplay,
  openAppsMenu,
  scrollPage,
  scrollToText,
  sleep,
  startHidpiDisplay,
} from "./shots.ts";

const NAMES = { a: "Alex", b: "Maya", c: "Jonas" } as const;
const BOTS = ["Priya", "Ben", "Zoe"];

/** Opens Settings on the sidebar tab whose label starts with `label`. */
async function openTab(label: string): Promise<void> {
  await openAllSettings();
  const tab = $(
    `//button[@role='tab'][.//span[contains(@class,'settings__tab-label')][starts-with(normalize-space(.),'${label}')]]`,
  );
  await tab.waitForClickable({ timeout: 10_000 });
  await tab.click();
  await $(
    `//h2[contains(@class,'settings-page__title')][normalize-space(.)='${label}']`,
  ).waitForDisplayed({
    timeout: 10_000,
  });
}

/** Opens the camera preview with the given background (the preview is a modal, closed between themes). */
function preview(choose: () => Promise<void>): () => Promise<void> {
  return async () => {
    await camera.openPreview();
    await choose();
    // The picture list loads its thumbnails after the pick.
    await sleep(2_500);
  };
}

const blur = (strength: "Light" | "Strong") => async () => {
  await camera.pickBackgroundMode("Blur");
  await camera.pickBlurStrength(strength);
};

const image = (name: string) => async () => {
  await camera.pickBackgroundMode("Image");
  await camera.pickImage(name);
};

(relayenv.relayAvailable() ? describe : describe.skip)(
  "0.7.0 screenshots: camera, screen share, files, activity log",
  function () {
    this.timeout(40 * 60_000);
    this.bail(true);

    let xvfb: ChildProcess | undefined;
    const bots: BackgroundBot[] = [];
    const moduleBots: ChildProcess[] = [];

    before(async function () {
      this.timeout(20 * 60_000);
      xvfb = await startHidpiDisplay();

      const { port } = await relayenv.startRelay("all");
      await startCamClient("a", port, { name: NAMES.a, env: { ...camera.MASK_ENV, ...HIDPI_ENV } });
      await startCamClient("b", port, { name: NAMES.b, env: camera.MASK_ENV });
      await startCamClient("c", port, {
        name: NAMES.c,
        env: { ...camera.MASK_ENV, ...relayenv.hookEnv({ sharePick: "screen" }) },
      });
      for (const name of BOTS) {
        const bot = botInBackground(["script"], { name, port, timeout: 2_400 });
        bot.send("sleep 2300");
        bots.push(bot);
      }
      moduleBots.push(moduleBot("DJ Nova", port, ["music", "history"], port + 50));
      moduleBots.push(moduleBot("Chronicle", port, ["history"], port + 51));

      await browser.setWindowSize(1440, 900);
      await on("b", () => browser.setWindowSize(1440, 900));
      await updateSettings({ cameraShow: "always" });
      await setRoomOpen(true);

      // Maya with a strong blur, Jonas in front of a built-in picture.
      await on("b", () => camera.turnOn(blur("Strong")));
      await on("c", () => camera.turnOn(image("Neon gaming room")));
      await on("b", () => sendText("Evening! Camera test before the raid, can you all see me?"));
      await on("c", () => sendText("Loud and clear. Nice blur, Maya."));
      await sendText("Looks great. Jonas, is that your new gaming room?");
      await on("c", () => sendText("I wish. It's one of the built-in backgrounds."));
      await waitTile(NAMES.b);
      await waitTile(NAMES.c);
      // The server's welcome text would cover the chat in every picture.
      const hide = $(".pinned-info__close");
      if (await hide.isExisting()) await hide.click();
    });

    after(async () => {
      for (const bot of bots) bot.stop();
      for (const child of moduleBots) child.kill();
      await apps.stopB();
      await apps.stopC();
      await relayenv.stopRelay();
      xvfb?.kill();
    });

    it("background effects in the preview: Light and Strong blur, a built-in picture, an own image", async () => {
      await both("camera-effects-blur-light", preview(blur("Light")));
      await both("camera-effects-blur-strong", preview(blur("Strong")));
      await both("camera-effects-background", preview(image("Space station above Earth")));

      await closeOverlays();
      const source = join(mkdtempSync(join(tmpdir(), "mumcord-shots-")), "my-room.jpg");
      copyFileSync(join(repoRoot, "e2e", "fixtures", "camera", "own-background.jpg"), source);
      await camera.openPreview();
      await camera.pickBackgroundMode("Image");
      const add = camera.previewDialog().$("button[aria-label='Add your own image']");
      await add.click();
      await onHidpiDisplay(() => files.pickInFileDialog("Add your own image", source));
      await browser.waitUntil(async () => (await add.getAttribute("aria-busy")) !== "true", {
        timeout: 20_000,
      });
      await both("camera-effects-own-image", preview(image("Your image 1")));

      await closeOverlays();
    });

    it("Settings → Camera, Screen sharing, Hotkeys and the file sharing card", async () => {
      const page = (label: string, scroll: () => Promise<void>) => async () => {
        await openTab(label);
        await scroll();
      };
      await both(
        "settings-camera-1",
        page("Camera", () => scrollPage(0)),
      );
      await both(
        "settings-camera-2",
        page("Camera", () => scrollPage(1)),
      );
      await both(
        "settings-screen-sharing-1",
        page("Screen sharing", () => scrollPage(0)),
      );
      await both(
        "settings-screen-sharing-2",
        page("Screen sharing", () => scrollPage(1)),
      );
      await both(
        "settings-hotkeys-camera",
        page("Voice & Audio", () => scrollToText("Turn camera on / off")),
      );
      await both(
        "settings-file-sharing",
        page("Privacy & Connections", () => scrollToText("File sharing")),
      );
      await closeOverlays();
    });

    it("camera room and its menus", async () => {
      // On air only now: the settings page's own preview needs the camera free.
      await camera.turnOn(blur("Light"));
      await camera.openRoom();
      await sleep(3_000);
      await both("camera-room");
      await both("camera-tile-menu", async () => {
        const options = $(`button[aria-label='Options for ${NAMES.b}']`);
        await options.moveTo();
        await options.click();
        await $("[role='menu']").waitForDisplayed({ timeout: 5_000 });
      });
      await both("camera-room-background-menu", async () => {
        await $("//button[starts-with(normalize-space(.),'Background:')]").click();
        await $(".cam-bg-popover").waitForDisplayed({ timeout: 5_000 });
      });
    });

    it("screen share: the start dialog", async () => {
      await both("screen-share-start-dialog", async () => {
        await openAppsMenu();
        await $(
          "//div[contains(@class,'chat-apps__menu')]//*[@role='menuitem'][starts-with(normalize-space(),'Share your screen')]",
        ).click();
        await $("//dialog//button[normalize-space()='Choose what to share']").waitForDisplayed({
          timeout: 20_000,
        });
      });
      await closeOverlays();
    });

    it("screen share with the camera strip and the viewer controls", async () => {
      await on("c", startScreenShare);
      const watch = $(
        `//div[contains(@class,'activity-banner')][contains(.,'${NAMES.c} is sharing their screen')]//button[normalize-space()='Watch']`,
      );
      await watch.waitForClickable({ timeout: 30_000 });
      await watch.click();
      const tab = $("#stage-tab-share");
      await tab.waitForClickable({ timeout: 20_000 });
      await tab.click();
      await $("section.cam-combined").waitForDisplayed({ timeout: 30_000 });
      await sleep(3_000);
      await both("screen-share-camera-strip", () => Promise.resolve(), true);
      await both(
        "screen-share-viewer-controls",
        async () => {
          await $(".media-frame:not(.plugin-stage)").moveTo();
          const quality = $(
            "//button[@aria-label='Quality' or normalize-space(.)='Quality' or starts-with(@aria-label,'Quality')]",
          );
          await quality.waitForClickable({ timeout: 10_000 });
          await quality.click();
        },
        true,
      );
      await browser.keys("Escape");
    });
  },
);
