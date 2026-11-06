// Spec 08, the viewer's choices and load: AC-8.9 (Hide video / Show video, Show cameras: Never),
// the automated part of AC-8.17 (a capped viewer gets fewer frames while the others keep theirs)
// and AC-8.23 (a screen share and the camera at the same time are two broadcasts).
//
// The relay has no API that lists broadcasts or their viewers, so "the relay shows B unbound"
// is read from A's side: A's camera view lists the viewers the relay reports to A, and A logs
// "broadcast rekeyed" when a viewer leaves (FR-8.57). "Received bytes stay at 0" is B's tile in
// the hidden state with no binding (the engine drops the subscriber link on hide).
//
// AC-8.17 needs `netns.sh` of mumpeer's testenv to throttle B to 1 Mbit/s, which needs root and
// network namespaces the CI runners don't give. Automated here instead: B's Data saver for A's
// tile caps it at 15 fps with the fps badge, while C keeps the full rate. Manual checklist:
//   - `sudo testenv/netns.sh throttle B 1mbit`: B's tiles drop to 15 or 7.5 fps with the fps
//     badge while C keeps 30 fps;
//   - on a 4-core 2020 laptop, blur at 720p30 runs at 30 fps with the level Auto picked (effect
//     time per frame in the debug panel, `MUMCORD_TEST_EFFECT_STATS=1`).
//
// Skips itself without a mumpeer checkout (see relay-uses.e2e.ts).
// Run: npm run e2e -- --spec e2e/specs/camera/viewer.e2e.ts
import { $, browser, expect } from "@wdio/globals";
import { app, apps, relayenv } from "../../helpers/index.ts";
import { startScreenShare } from "../../helpers/chat-ui.ts";
import { personRow } from "../../helpers/voice-ui.ts";
import {
  NAMES,
  cameraView,
  clientLog,
  on,
  ownSession,
  serverId,
  sessionOf,
  setRoomOpen,
  sleep,
  startCamClient,
  tileOf,
  turnCameraOff,
  turnCameraOn,
  updateSettings,
  waitTile,
} from "./flows.ts";

const REKEYED = "broadcast rekeyed";
const rekeys = (): number => clientLog("a").split("\n").filter((l) => l.includes(REKEYED)).length;

/** Waits until A's camera lists `session` as a viewer (`present`) or not. */
async function waitViewer(session: number, present: boolean, timeout = 15_000): Promise<void> {
  let viewers: number[] = [];
  await browser
    .waitUntil(
      async () => {
        viewers = (await cameraView())?.viewers ?? [];
        return viewers.includes(session) === present;
      },
      { timeout, interval: 250 },
    )
    .catch(() => {
      throw new Error(`A's viewers are ${JSON.stringify(viewers)}; want ${String(session)} ${present ? "in" : "out"}`);
    });
}

(relayenv.relayAvailable() ? describe : describe.skip)("camera viewer choices (AC-8.9, 8.17, 8.23)", function () {
  this.timeout(900_000);
  this.bail(true);

  let bob = 0;
  let carol = 0;

  before(async () => {
    const { port } = await relayenv.startRelay("all");
    await startCamClient("a", port, { env: relayenv.hookEnv({ sharePick: "screen" }) });
    // Every share goes out as a relay broadcast.
    await updateSettings({ p2pMode: "relayOnly" });
    await startCamClient("b", port);
    await startCamClient("c", port);
    for (const id of ["b", "c"] as const) {
      await on(id, async () => {
        await updateSettings({ cameraShow: "always" });
        await setRoomOpen(true);
      });
    }
    bob = await sessionOf(NAMES.b);
    carol = await sessionOf(NAMES.c);
  });

  after(async () => {
    await apps.stopB();
    await apps.stopC();
    await relayenv.stopRelay();
  });

  it("AC-8.9: B hides A's video, A drops B and rekeys; Show video brings it back", async () => {
    await turnCameraOn();
    await on("b", () => waitTile(NAMES.a));
    await on("c", () => waitTile(NAMES.a));
    await waitViewer(bob, true);
    await waitViewer(carol, true);
    const rekeysBefore = rekeys();

    const alice = await ownSession();
    await on("b", async () => {
      await app.invoke("camera_tile_set_hidden", { serverId: await serverId(), sharer: alice, hidden: true });
      await waitTile(NAMES.a, "hidden");
    });
    await waitViewer(bob, false);
    await browser.waitUntil(async () => Promise.resolve(rekeys() > rekeysBefore), {
      timeout: 15_000,
      timeoutMsg: "A never rekeyed after B left",
    });
    // B stays unbound for 10 s while C keeps watching.
    await sleep(10_000);
    expect((await cameraView())?.viewers).not.toContain(bob);
    await on("b", async () => {
      expect((await tileOf(NAMES.a))?.state).toBe("hidden");
    });
    await on("c", () => waitTile(NAMES.a));

    await on("b", async () => {
      const show = $(`//div[contains(@class,'cam-tile')][starts-with(@aria-label,'${NAMES.a}')]//button[normalize-space()='Show video']`);
      await show.waitForClickable({ timeout: 10_000 });
      await show.click();
      await waitTile(NAMES.a);
    });
    await waitViewer(bob, true);
  });

  it("AC-8.9: Show cameras: Never binds B to no camera; the tree icons stay", async () => {
    await on("b", () => updateSettings({ cameraShow: "never" }));
    await waitViewer(bob, false);
    await sleep(5_000);
    expect((await cameraView())?.viewers).not.toContain(bob);
    await on("b", async () => {
      const tile = await tileOf(NAMES.a);
      expect(tile === undefined || tile.state !== "playing").toBe(true);
      await personRow(NAMES.a).$(".camera-tree-icon").waitForExist({ timeout: 10_000 });
      await updateSettings({ cameraShow: "always" });
      await setRoomOpen(true);
      await waitTile(NAMES.a);
    });
    await waitViewer(bob, true);
  });

  it("AC-8.17 (automated part): B's Data saver caps A's tile at 15 fps; C keeps the full rate", async () => {
    const alice = await ownSession();
    await on("b", async () => {
      await app.invoke("camera_tile_set_quality", { serverId: await serverId(), sharer: alice, quality: "dataSaver" });
      await browser.waitUntil(
        async () => {
          const tile = await tileOf(NAMES.a);
          return tile?.dataSaver === true && tile.fpsBadge !== null && tile.fpsBadge <= 15;
        },
        { timeout: 20_000, interval: 500, timeoutMsg: "B's tile of A never showed the capped fps badge" },
      );
      await $(`//div[contains(@class,'cam-tile')][starts-with(@aria-label,'${NAMES.a}')]//*[contains(concat(' ',@class,' '),' cam-tile__badge ')][contains(.,'fps')]`).waitForDisplayed({ timeout: 10_000 });
    });
    await on("c", async () => {
      const tile = await waitTile(NAMES.a);
      expect(tile.dataSaver).toBe(false);
      // No badge: C gets the full frame rate.
      expect(tile.fpsBadge === null || tile.fpsBadge > 15).toBe(true);
    });
    await on("b", async () => {
      await app.invoke("camera_tile_set_quality", { serverId: await serverId(), sharer: alice, quality: "auto" });
    });
  });

  it("AC-8.23: a screen share and the camera at the same time are two broadcasts; B sees both", async () => {
    const camera = (await cameraView())?.broadcast;
    expect(camera).toMatch(/^[0-9a-f]{16}$/);
    await startScreenShare();
    const alice = await ownSession();
    await on("b", async () => {
      const watch = $(
        `//div[contains(@class,'activity-banner')][contains(.,'${NAMES.a} is sharing their screen')]//button[normalize-space()='Watch']`,
      );
      await watch.waitForClickable({ timeout: 30_000, timeoutMsg: "B never saw A's share" });
      await watch.click();
      let path: string | undefined;
      await browser
        .waitUntil(
          async () => {
            const id = await serverId();
            path = (await app.invoke<{ path: string } | null>("screen_share_watch_stats", { serverId: id, session: alice }))?.path;
            return path === "broadcast";
          },
          { timeout: 60_000, interval: 1_000 },
        )
        .catch(() => {
          throw new Error(`B watches A's screen through ${String(path)}, want the broadcast`);
        });
      // The camera keeps its own broadcast next to the share.
      await waitTile(NAMES.a);
    });
    expect((await cameraView())?.broadcast).toBe(camera);
    await $(".sharer-bar button[aria-label='Stop sharing']").click();
    await turnCameraOff();
  });
});
