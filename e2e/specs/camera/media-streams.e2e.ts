// Spec 08 §4.14, the camera path matrix and its fallbacks (FR-8.94, FR-8.95) in the running
// client: which path each viewer gets, what the room shows per viewer, and how a viewer that
// cannot be served is refused.
//
//   AC-8.32  on the direct path hiding, leaving and blocking close the viewer's stream, and a
//            stale camera session id is refused without a crash.
//   AC-8.30  with the new relay (profile `all`) the camera uses its relay broadcast and each
//            viewer's path is shown as Relay or Direct; `direct_only` opens no broadcast.
//   AC-8.31  (sender half) a viewer whose hello lacks `media-v2` gets a `mumcord.camera` `needs`,
//            is admitted to nothing and gets no second broadcast — on the new relay and with
//            `direct_only`.
//   AC-8.33  a room mixes a relay camera and a direct camera of two senders.
//
// AC-8.29 (two 0.9.0 clients with no relay watch each other over the direct path) lives in
// `camera/media-streams-direct.e2e.ts`: it asserts there is no relay, which a shard that starts the
// relay environment cannot offer. Keeping the two apart is what lets both run.
//
// Not automatable in this environment, so covered elsewhere or reported:
//   - AC-8.31's 0.8.0 build (the tile text "This camera needs Mumcord 0.9.0", no second
//     `cam-join`, no video): the E2E image carries only the current build. Reported as "not
//     verified" instead of worked around; the sender half is automated here when the relay
//     announces media version 2.
//   - The pinned mumpeer (`MUMPEER_REF`) still speaks the old format: `/v1/status` has no
//     `"media": 2`. AC-8.30 then checks the old-relay half (0.9.0 viewers on the direct path,
//     the old-format broadcast still open). AC-8.31 and AC-8.33 need the new format, so they
//     skip until the pin moves. The 0.1.0 image (`MUMCORD_E2E_OLD_MUMPEER_IMAGE`) has no
//     broadcasts at all; an old-format broadcast serving a 0.8.x viewer stays in
//     `crates/engine/tests/live_camera_paths.rs`.
//   - The 8-viewer direct and 25-viewer relay `full`: eight real direct viewers need eight more
//     app instances, and the bot admits relay viewers by certificate, which a scripted client has
//     not. Covered by `camera::direct::tests`, `camera::admit::tests` and the live tests.
//   - AC-8.38 needs `sudo testenv/netns.sh`; AC-8.34 to 8.37 are the relay media profile itself
//     (D1–D3 work), not the path matrix. `/v1/status` has no broadcast list and the relay logs no
//     broadcast ids, so "no second broadcast" is read from A's own view: one broadcast id,
//     unchanged, and an empty viewer list.
//
// Skips itself without a mumpeer checkout (see relay-uses.e2e.ts).
// Run: npm run e2e -- --spec e2e/specs/camera/media-streams.e2e.ts
import { $, $$, browser, expect } from "@wdio/globals";
import { app, apps, relayenv, testenv } from "../../helpers/index.ts";
import { reconnect } from "../../helpers/voice-ui.ts";
import {
  NAMES,
  RawClient,
  type CameraViewer,
  cameraId,
  cameraMessages,
  cameraView,
  clientLog,
  on,
  serverId,
  sessionOf,
  setRoomOpen,
  startCamClient,
  turnCameraOff,
  turnCameraOn,
  updateSettings,
  waitNoTile,
  waitOwnViewer,
  waitTile,
} from "./flows.ts";

/** How the room names the own camera's path (camera.json `room.*`, FR-8.62). */
const DIRECT_CHIP = "Direct · end-to-end encrypted";
const RELAY_CHIP = "Relay · end-to-end encrypted";

/** A camera tile must arrive within 3 s (FR-8.60, AC-8.29); the poll interval adds a little. */
const TILE_MS = 3_000;

/** The path chip of the camera room header: Relay, Direct or both (FR-8.62). */
function pathChip() {
  return $(".cam-room__head .cam-chip[data-tone='ok']");
}

/** Opens A's viewer list in the room bar and returns the path text of each row (FR-8.62). */
async function viewerListPaths(): Promise<string[]> {
  const button = $(".cam-viewers button.cam-btn");
  await button.waitForClickable({ timeout: 20_000 });
  if ((await button.getAttribute("aria-expanded")) !== "true") await button.click();
  const rows = await $$(".cam-viewers__path");
  const paths: string[] = [];
  for (let i = 0; i < rows.length; i++) paths.push((await rows[i].getText()).trim());
  return paths;
}

/** Waits until the own camera no longer lists `session` as a viewer. */
async function waitNoOwnViewer(session: number, timeout = 20_000): Promise<void> {
  let viewers: CameraViewer[] = [];
  await browser
    .waitUntil(
      async () => {
        viewers = (await cameraView())?.viewers ?? [];
        return !viewers.some((v) => v.session === session);
      },
      { timeout, interval: 250 },
    )
    .catch(() => {
      throw new Error(`the own viewers still are ${JSON.stringify(viewers)}`);
    });
}

/** The `mumcord.camera` messages of type `t` that the current client sent. */
async function sentCamera(t: string): Promise<Record<string, unknown>[]> {
  return (await cameraMessages())
    .filter((m) => m.direction === "sent" && m.fields?.t === t)
    .map((m) => m.fields ?? {});
}

/**
 * A 0.8.x-style scripted viewer: a hand-made Mumble client whose hello names `camera` but not
 * `media-v2` (the 0.8.0 hello of P0.5), so its `cam-join` carries no `mv: 2`. That is the one
 * capability FR-8.95 is about: 0.8.x has no direct camera path and cannot read the new profile.
 * Returns the camera session id it asked for.
 */
async function scriptedJoin(viewer: RawClient, owner: string, staleId?: string): Promise<string> {
  await viewer.joinChannelOf(owner);
  const target = viewer.sessionOf(owner);
  // Without this hello the sender does not even count it as a camera peer (FR-2.75).
  const hello = Buffer.from(JSON.stringify({ app: "mumcord", v: 1, features: ["camera"] }), "utf8");
  viewer.pluginData(target, "mumcord.hello", hello);
  // The sender answers a newcomer with its `on`, which names the camera session (§6.5).
  const id = staleId ?? cameraId(await viewer.cameraDocument("on"));
  // Bovnar text as in §6.5; no `mv`, which is the whole point.
  viewer.pluginData(
    target,
    "mumcord.camera",
    Buffer.from(`.v = 1; .t = "cam-join"; .id = "${id}"; .codecs = "av1,vp9";`, "utf8"),
  );
  viewer.ping();
  return id;
}

(relayenv.relayAvailable() ? describe : describe.skip)(
  "camera path matrix and fallbacks over the relay (AC-8.30 to 8.33)",
  function () {
    this.timeout(900_000);
    this.bail(true);

    let port = 0;
    let bob = 0;
    let carol = 0;
    /** True once AC-8.30 has seen a relay that announces media version 2 (FR-8.94). */
    let freshRelay = false;

    after(async () => {
      await apps.stopB();
      await apps.stopC();
      await relayenv.stopRelay();
    });

    it("AC-8.32: hiding, leaving and blocking close the viewer's direct stream; a stale session id is refused", async () => {
      // This cell is first and stands on its own: a plain server, no relay, three clients already
      // watching A's camera over the direct path (FR-8.96). The relay cells below start their
      // own server.
      await relayenv.stopRelay();
      await testenv.up("v15");
      const plain = testenv.ports.v15;
      await startCamClient("a", plain);
      await startCamClient("b", plain);
      await startCamClient("c", plain);
      for (const id of ["a", "b", "c"] as const) await on(id, () => setRoomOpen(true));
      await turnCameraOn();
      await on("b", () => waitTile(NAMES.a, "playing", TILE_MS));
      await on("c", () => waitTile(NAMES.a, "playing", TILE_MS));
      bob = await sessionOf(NAMES.b);
      carol = await sessionOf(NAMES.c);

      // B hides A's video: the link closes and A drops B from its viewer list (FR-8.57).
      await on("b", async () => {
        await app.invoke("camera_tile_set_hidden", {
          serverId: await serverId(),
          sharer: await sessionOf(NAMES.a),
          hidden: true,
        });
        await waitTile(NAMES.a, "hidden");
      });
      await waitNoOwnViewer(bob);
      // Show video brings the tile and the stream back.
      await on("b", async () => {
        const show = $(
          `//div[contains(@class,'cam-tile')][starts-with(@aria-label,'${NAMES.a}')]//button[normalize-space()='Show video']`,
        );
        await show.waitForClickable({ timeout: 10_000 });
        await show.click();
        // Show sends cam-join again (FR-8.57). A viewer may send that at most once per 10 s
        // (§6.5), so this is not the 3 s camera-start budget.
        await waitTile(NAMES.a, "playing");
      });
      expect((await waitOwnViewer(bob)).path).toBe("direct");

      // Blocking the camera's sender ends the tile on B's side and the stream on A's (FR-8.59).
      await on("b", async () => {
        await app.invoke("block_user", {
          serverId: await serverId(),
          session: await sessionOf(NAMES.a),
        });
        await waitNoTile(NAMES.a);
      });
      await waitNoOwnViewer(bob);

      // Leaving voice closes the stream too: the testenv server has only the root channel, so
      // "leaving the channel" is leaving voice, as in stop.e2e.ts.
      await on("c", () => app.invoke("voice_leave"));
      await waitNoOwnViewer(carol);
      await on("c", reconnect);

      // A stale camera session id in a `cam-join` is refused: no admission, no `needs`, no crash
      // (FR-8.92, FR-8.95).
      await app.invoke("packet_capture_background", { enabled: true });
      await app.invoke("packet_capture_clear");
      const stale = new RawClient("Stale");
      await stale.connect(testenv.ports.v15);
      try {
        await scriptedJoin(stale, NAMES.a, "ffffffffffffffff");
        await browser.pause(2_000);
        const viewers = (await cameraView())?.viewers ?? [];
        expect(viewers.some((v) => v.session === stale.session)).toBe(false);
        expect(await sentCamera("needs")).toEqual([]);
      } finally {
        stale.close();
      }
      expect((await cameraView())?.state).toBe("onAir");
      await turnCameraOff();
    });

    it("AC-8.30: with the new relay the camera uses its broadcast, and the viewer list shows each path", async () => {
      ({ port } = await relayenv.startRelay("all"));
      // The relay environment has its own server: both clients move over to it.
      await startCamClient("a", port);
      await startCamClient("b", port);
      for (const id of ["a", "b"] as const) await on(id, () => setRoomOpen(true));
      bob = await sessionOf(NAMES.b);
      // Profile `all` on a relay that announces `mv` is the new format. The pinned image does
      // not, and then 0.9.0 viewers stay on the direct path while the old broadcast stays up
      // for 0.8.x (FR-8.94).
      freshRelay = ((await relayenv.relayStatus()).media as number | undefined) === 2;

      const view = await turnCameraOn();
      expect(view.broadcast).toMatch(/^[0-9a-f]{16}$/);
      await on("b", () => waitTile(NAMES.a, "playing", TILE_MS));
      if (freshRelay) {
        expect((await waitOwnViewer(bob)).path).toBe("relay");
        expect(await pathChip().getText()).toBe(RELAY_CHIP);
        expect(await viewerListPaths()).toEqual(["Relay"]);
      } else {
        expect((await waitOwnViewer(bob)).path).toBe("direct");
        expect(await pathChip().getText()).toBe(DIRECT_CHIP);
        expect(await viewerListPaths()).toEqual(["Direct"]);
      }

      // `direct_only` never opens a relay broadcast; the same viewer then gets a stream.
      await turnCameraOff();
      await updateSettings({ p2pMode: "directOnly" });
      const direct = await turnCameraOn();
      expect(direct.broadcast).toBeNull();
      await on("b", () => waitTile(NAMES.a, "playing", TILE_MS));
      expect((await waitOwnViewer(bob)).path).toBe("direct");
      expect(await pathChip().getText()).toBe(DIRECT_CHIP);
      expect(await viewerListPaths()).toEqual(["Direct"]);
      await updateSettings({ p2pMode: "relayFirst" });
    });

    it("AC-8.31: a viewer without `media-v2` gets `needs` and no stream, on the relay and with `direct_only`", async function () {
      // On an old relay the same viewer is served by the old-format broadcast and gets no
      // `needs` (AC-8.31). That half needs a 0.8.0 build, which this image does not carry.
      if (!freshRelay) this.skip();
      await app.invoke("packet_capture_background", { enabled: true });
      await app.invoke("packet_capture_clear");
      const broadcast = (await turnCameraOn()).broadcast;
      expect(broadcast).toMatch(/^[0-9a-f]{16}$/);
      if (broadcast === null) throw new Error("a new relay must give the camera a broadcast");

      // A new-format camera on the new relay: the scripted viewer cannot read it.
      const streams = (): number =>
        (clientLog("a").match(/camera direct stream opened/g) ?? []).length;
      const streamsBefore = streams();
      const olaf = new RawClient("Olaf");
      await olaf.connect(port);
      try {
        await scriptedJoin(olaf, NAMES.a, broadcast);
        // The sender answers `needs` with the camera session id and admits it to nothing.
        await browser.waitUntil(async () => (await sentCamera("needs")).length > 0, {
          timeout: 20_000,
          timeoutMsg: "A never answered the 0.8.x-style viewer with needs",
        });
        expect(String((await sentCamera("needs"))[0]?.id)).toBe(broadcast);
        expect((await cameraView())?.viewers.some((v) => v.session === olaf.session)).toBe(false);
        // One broadcast, unchanged: no second relay broadcast for it (FR-8.95).
        expect((await cameraView())?.broadcast).toBe(broadcast);
        // No `STREAM_CAMERA` for it either.
        expect(streams()).toBe(streamsBefore);
        // And the viewer itself reads the refusal (§6.5).
        expect(await olaf.cameraDocument("needs")).toContain(broadcast);
      } finally {
        olaf.close();
      }

      // The same with `direct_only`: no broadcast at all, so the session id comes from A's `on`,
      // which A re-announces to a newcomer.
      await turnCameraOff();
      await updateSettings({ p2pMode: "directOnly" });
      await turnCameraOn();
      const olga = new RawClient("Olga");
      await olga.connect(port);
      try {
        const id = await scriptedJoin(olga, NAMES.a);
        expect(await olga.cameraDocument("needs")).toContain(id);
        expect((await cameraView())?.viewers.some((v) => v.session === olga.session)).toBe(false);
      } finally {
        olga.close();
      }
      await updateSettings({ p2pMode: "relayFirst" });
    });

    it("AC-8.33: a room mixes a relay camera and a direct camera of two senders", async function () {
      // Both 0.9.0 viewers are on the direct path until the relay speaks the new format, so
      // this cell has no relay camera to mix in.
      if (!freshRelay) this.skip();
      await turnCameraOff();
      // A keeps the new relay's broadcast, B runs `direct_only` with its own streams.
      await on("b", async () => {
        await updateSettings({ p2pMode: "directOnly" });
        await setRoomOpen(true);
      });
      await turnCameraOn();
      await on("b", () => turnCameraOn());
      await startCamClient("c", port);
      await on("c", async () => {
        await setRoomOpen(true);
        await waitTile(NAMES.a, "playing", TILE_MS);
        await waitTile(NAMES.b, "playing", TILE_MS);
      });
      carol = await sessionOf(NAMES.c);
      await on("b", async () => {
        expect((await waitOwnViewer(carol)).path).toBe("direct");
        expect((await cameraView())?.broadcast).toBeNull();
      });
      expect((await waitOwnViewer(carol)).path).toBe("relay");
      expect((await cameraView())?.broadcast).toMatch(/^[0-9a-f]{16}$/);
    });
  },
);
