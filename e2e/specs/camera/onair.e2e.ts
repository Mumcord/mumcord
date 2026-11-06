// Spec 08, preview and going on air (AC-8.3, AC-8.4) in the running client against the mumpeer
// test environment (profile `all`). A = Alice shares her test camera (MUMCORD_TEST_CAMERA), B = Bob
// and C = Carol watch in the same channel. All three are registered: the relay bot gives relay
// tokens to registered users only.
//
//   AC-8.3  A opens the preview → the test camera picture shows; the engine has no broadcast and B
//           sees neither a tile nor a camera icon for A until A presses Turn on camera. Cancel →
//           the capture stops (camera state off).
//   AC-8.4  A turns the camera on → within 3 s B and C have the camera icon at A in their trees and
//           A's tile, whose canvas then shows the test camera's pixels; A sees the pressed (red)
//           user-panel button, "● Camera on", "● Live" on the self-view, the own tree icon,
//           "Camera on" in the room bar, and the live region says "Your camera is on".
//           Turning off removes the tile for B.
//
// Manual (not on CI): AC-8.3 "/v1/status and the bot show no broadcast" and "B's packet inspector
// shows no mumcord.camera message" (the relay status has no broadcast list and the inspector has no
// test hook; the engine's broadcast id stands in), "debug panel: camera closed", and AC-8.4
// "O (plain Mumble client) sees no new chat line".
//
// Skips itself without a mumpeer checkout.
import { browser, $, expect } from "@wdio/globals";
import { apps, relayenv } from "../../helpers/index.ts";
import * as camera from "../../helpers/camera.ts";
import { personRow } from "../../helpers/voice-ui.ts";

(relayenv.relayAvailable() ? describe : describe.skip)("camera preview and on air (spec 08, AC-8.3, 8.4)", function () {
  this.timeout(900_000);
  this.bail(true);

  after(async () => {
    await apps.stopC();
    await apps.stopB();
  });

  it("setup: Alice, Bob and Carol in the channel of a relay that allows cameras", async () => {
    const { port } = await relayenv.startRelay("all");
    await camera.startA("Alice", port);
    await camera.startB("Bob", port);
    await camera.startC("Carol", port);
    for (const name of ["Bob", "Carol"]) await personRow(name).waitForDisplayed({ timeout: 30_000 });
    await camera.expectCameraBlock(null);
  });

  it("AC-8.3: the preview shows the test camera and sends nothing", async () => {
    await camera.openPreview();
    const view = await camera.cameraView();
    expect(view?.state).toBe("preview");
    expect(view?.broadcast).toBeNull();
    // Give a wrongly sent announcement time to arrive before checking that none did.
    await browser.pause(3000);
    await apps.onB(async () => {
      expect(await camera.hasTreeCamera("Alice")).toBe(false);
      expect(await camera.tile("Alice").isExisting()).toBe(false);
    });
  });

  it("AC-8.3: Cancel stops the capture", async () => {
    await camera.cancelPreview();
    await camera.waitCameraState("off", 5000);
  });

  it("AC-8.4: turning on shows A's tile and icon to B and C within 3 s", async () => {
    await camera.recordAnnouncements();
    await camera.turnOn();
    const onAt = Date.now();
    for (const viewer of [apps.onB, apps.onC]) {
      await viewer(async () => {
        await browser.waitUntil(() => camera.hasTreeCamera("Alice"), {
          timeout: Math.max(500, 3000 - (Date.now() - onAt)),
          interval: 100,
          timeoutMsg: "no camera icon at Alice within 3 s",
        });
      });
    }
    for (const viewer of [apps.onB, apps.onC]) {
      await viewer(async () => {
        await camera.openRoom();
        await camera.tile("Alice").waitForDisplayed({ timeout: 3000, timeoutMsg: "no tile named Alice" });
        await camera.waitTilePicture("Alice");
        // The pixels are the test camera's: the head differs from the bars above it.
        const [head, bar] =
          (await camera.samplePixels(camera.tileCanvas("Alice"), [camera.PERSON_POINTS[0], [0.5, 0.05]])) ?? [];
        expect(camera.meanDiff([head], [bar])).toBeGreaterThan(20);
      });
    }
  });

  it("AC-8.4: A sees every on-air marker and hears the announcement", async () => {
    const button = camera.cameraButton();
    expect(await button.getAttribute("aria-pressed")).toBe("true");
    expect(await button.getAttribute("data-onair")).not.toBeNull();
    await expect($(".camera-on-line")).toHaveText(expect.stringContaining("Camera on"));
    expect(await camera.hasOwnTreeCamera("Alice")).toBe(true);
    await camera.openRoom();
    const self = $("div.cam-tile[aria-label^='You']");
    await self.waitForDisplayed({ timeoutMsg: "A has no self-view tile" });
    await expect(self.$(".cam-tile__badge[data-tone='live']")).toHaveText(expect.stringContaining("Live"));
    await expect($(".cam-room__bar .cam-btn--onair")).toHaveText(expect.stringContaining("Camera on"));
    expect(await camera.announcements()).toContain("Your camera is on");
  });

  it("AC-8.4: turning off removes the tile for B", async () => {
    await camera.turnOff();
    await apps.onB(async () => {
      await camera.tile("Alice").waitForExist({ reverse: true, timeout: 10_000 });
      expect(await camera.hasTreeCamera("Alice")).toBe(false);
    });
  });
});
