// Spec 08, background effects and own images (AC-8.5, AC-8.6) in the running client against the
// mumpeer test environment (profile `all`). A = Alice runs the test camera with the fixture
// segmentation mask (MUMCORD_TEST_SEGMENT_MASK=fixture: the test camera's known person outline
// replaces the model), B = Bob watches. Pixels are compared with helpers/camera.ts: background
// points next to bar edges must change against the "None" picture, person points must not.
//
//   AC-8.5  Blur, then a built-in image → in A's preview and in B's tile the background changes and
//           the person stays; with the real model (no mask hook) the background of A's preview
//           still changes measurably.
//   AC-8.6  A adds an own image (a JPEG with EXIF) and uses it → the preview and B's tile show it
//           behind the person; it is stored in A's data folder `backgrounds/` as a re-encoded JPEG
//           with a new name and without the EXIF block.
//
// Manual (not on CI): AC-8.6 "no mumcord.peer, mumcord.camera or relay control message contains
// the image bytes, its name or its path" needs the packet inspector and the relay's log.
//
// Skips itself without a mumpeer checkout.
import { copyFileSync, mkdtempSync, readdirSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { browser, $, expect } from "@wdio/globals";
import { app, apps, relayenv, repoRoot } from "../../helpers/index.ts";
import * as camera from "../../helpers/camera.ts";
import { pickInFileDialog } from "../../helpers/files-ui.ts";
import { personRow, reconnect } from "../../helpers/voice-ui.ts";

const BUILTIN = "Warm light waves";
const OWN_NAME = "secret-holiday.jpg";
const EXIF_MARKER = "MUMCORD-EXIF-MARKER";
/** The colour of the own-image fixture (e2e/fixtures/camera/own-background.jpg). */
const ORANGE: camera.Rgb = [255, 128, 0];

type Regions = Awaited<ReturnType<typeof camera.sampleRegions>>;

/** Turns A's camera on with the background `choose` picks, and waits for B's picture. */
async function onAirWith(choose: () => Promise<void>): Promise<void> {
  await camera.turnOn(choose);
  await apps.onB(async () => {
    await camera.openRoom();
    await camera.waitTilePicture("Alice");
  });
}

(relayenv.relayAvailable() ? describe : describe.skip)("camera effects and own images (spec 08, AC-8.5, 8.6)", function () {
  this.timeout(900_000);
  this.bail(true);

  let previewNone: Regions;
  let tileNone: Regions;

  after(async () => {
    await apps.stopB();
  });

  it("setup: Alice (fixture mask) and Bob in the channel of a relay that allows cameras", async () => {
    const { port } = await relayenv.startRelay("all");
    await camera.startA("Alice", port, camera.MASK_ENV);
    await camera.startB("Bob", port);
    await personRow("Bob").waitForDisplayed({ timeout: 30_000 });
    await camera.expectCameraBlock(null);
  });

  it("AC-8.5: Blur and a built-in image change the preview's background, not the person", async () => {
    await camera.openPreview();
    expect(await camera.effectsUnavailable()).toBeNull();
    await camera.pickBackgroundMode("None");
    previewNone = await camera.sampleRegions(camera.PREVIEW_CANVAS);
    await camera.pickBackgroundMode("Blur");
    await camera.pickBlurStrength("Strong");
    await camera.waitBackgroundChanged(camera.PREVIEW_CANVAS, previewNone);
    await camera.pickBackgroundMode("Image");
    await camera.pickImage(BUILTIN);
    await camera.waitBackgroundChanged(camera.PREVIEW_CANVAS, previewNone, { minBackground: 25 });
    await camera.pickBackgroundMode("None");
    await camera.turnOnFromPreview();
    await apps.onB(async () => {
      await camera.openRoom();
      await camera.waitTilePicture("Alice");
      tileNone = await camera.sampleRegions(camera.tileCanvas("Alice"));
    });
    await camera.turnOff();
  });

  it("AC-8.5: B's tile shows the blurred background", async () => {
    await onAirWith(async () => {
      await camera.pickBackgroundMode("Blur");
      await camera.pickBlurStrength("Strong");
    });
    await apps.onB(() => camera.waitBackgroundChanged(camera.tileCanvas("Alice"), tileNone));
    await camera.turnOff();
  });

  it("AC-8.5: B's tile shows the built-in image", async () => {
    await onAirWith(async () => {
      await camera.pickBackgroundMode("Image");
      await camera.pickImage(BUILTIN);
    });
    await apps.onB(() =>
      camera.waitBackgroundChanged(camera.tileCanvas("Alice"), tileNone, { minBackground: 25 }),
    );
    await camera.turnOff();
  });

  it("AC-8.6: an own image is stored re-encoded without EXIF and shows behind the person", async () => {
    const dir = mkdtempSync(join(tmpdir(), "mumcord-cam-"));
    const source = join(dir, OWN_NAME);
    copyFileSync(join(repoRoot, "e2e", "fixtures", "camera", "own-background.jpg"), source);
    expect(readFileSync(source).includes(EXIF_MARKER)).toBe(true);

    await camera.openPreview();
    await camera.pickBackgroundMode("Image");
    const add = camera.previewDialog().$("button[aria-label='Add your own image']");
    await add.click();
    await pickInFileDialog("Add your own image", source);
    await browser.waitUntil(async () => (await add.getAttribute("aria-busy")) !== "true", {
      timeout: 20_000,
      timeoutMsg: "the image import never finished",
    });
    await camera.pickImage("Your image 1");

    const folder = join(app.profile(), "data", "app.mumcord.client", "backgrounds");
    const stored = readdirSync(folder);
    expect(stored).toHaveLength(1);
    expect(stored[0]).not.toContain("secret-holiday");
    const bytes = readFileSync(join(folder, stored[0]));
    expect([...bytes.subarray(0, 2)]).toEqual([0xff, 0xd8]);
    expect(bytes.includes("Exif")).toBe(false);
    expect(bytes.includes(EXIF_MARKER)).toBe(false);

    // Choosing "Image" first shows the last built-in image, which already differs from "None":
    // wait for the own image's colour, not just for any change.
    await camera.waitBackgroundChanged(camera.PREVIEW_CANVAS, previewNone, {
      minBackground: 25,
      near: { colour: ORANGE, max: 30 },
    });
    await camera.turnOnFromPreview();
    await apps.onB(async () => {
      await camera.openRoom();
      await camera.waitTilePicture("Alice");
      await camera.waitBackgroundChanged(camera.tileCanvas("Alice"), tileNone, {
        minBackground: 25,
        near: { colour: ORANGE, max: 40 },
      });
    });
    await camera.turnOff();
  });

  it("AC-8.5: with the real model the preview's background still changes", async () => {
    await app.restart({ env: camera.CAMERA_ENV });
    // The relay server's root channel is not "Lobby": reconnect without naming a channel.
    await reconnect();
    await camera.joinVoice();
    await camera.expectCameraBlock(null, 90_000);
    await camera.openPreview();
    expect(await camera.effectsUnavailable()).toBeNull();
    await camera.pickBackgroundMode("None");
    const none = await camera.sampleRegions(camera.PREVIEW_CANVAS);
    await camera.pickBackgroundMode("Image");
    await camera.pickImage(BUILTIN);
    // The model decides where the drawn person ends, so only the background is checked.
    await camera.waitBackgroundChanged(camera.PREVIEW_CANVAS, none, {
      minBackground: 15,
      maxPerson: 255,
      timeout: 60_000,
    });
    await camera.cancelPreview();
    await expect($("dialog.cam-dialog")).not.toBeDisplayed();
  });
});
