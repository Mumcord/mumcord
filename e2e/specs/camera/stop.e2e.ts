// Spec 08, the stop rules and the relay's blindness: AC-8.12 (a viewer who left can't decrypt
// new frames with the keys it kept), AC-8.10 (leaving voice, disconnecting and quitting each stop
// the camera at once, and the tile disappears for the others) and AC-8.11 (the relay goes down
// while the camera is on).
//
// The relay environment's server has only the Root channel, so "A leaves the channel" is A
// leaving voice (the same stop rule, FR-8.21); moving to another channel is covered by the engine
// live test `crates/engine/tests/live_camera_stop.rs`. The bot deleting the broadcast shows as
// the tile disappearing for the others (the relay has no API that lists broadcasts).
//
// Manual (needs root or real hardware):
//   - AC-8.12 first half: a packet capture of a subscriber stream on the relay's port
//     (`sudo tcpdump -i lo udp port <relayPort> -w cam.pcap`) holds no decodable AV1 or VP9
//     bitstream (no OBU or VP9 frame headers in the clear; `ffprobe` finds no stream);
//   - AC-8.10 and AC-8.25: the camera light goes off within 1 s on every stop rule;
//   - AC-8.11: A's packet inspector and network log show no direct-connection attempt after the
//     relay went down (automated below for the plugin data A sends: no `mumcord.peer` pair
//     request).
//
// Skips itself without a mumpeer checkout (see relay-uses.e2e.ts).
// Run: npm run e2e -- --spec e2e/specs/camera/stop.e2e.ts
import { browser, expect } from "@wdio/globals";
import { app, apps, relayenv } from "../../helpers/index.ts";
import { runOk } from "../../helpers/run.ts";
import { joinIfReadOnly, reconnect } from "../../helpers/voice-ui.ts";
import {
  NAMES,
  captured,
  clientLog,
  on,
  serverId,
  setRoomOpen,
  sleep,
  startCamClient,
  startCapture,
  turnCameraOn,
  waitCameraState,
  waitNoTile,
  waitText,
  waitTile,
} from "./flows.ts";

/** A stop rule must turn the camera off within 1 s (FR-8.21); the poll itself takes a moment. */
const STOP_MS = 1_500;

(relayenv.relayAvailable() ? describe : describe.skip)("camera stop rules (AC-8.10 to 8.12)", function () {
  this.timeout(900_000);
  this.bail(true);

  before(async () => {
    const { port } = await relayenv.startRelay("all");
    await startCamClient("a", port);
    await startCamClient("b", port);
    // C keeps the broadcast keys after it leaves and tries them on what still arrives (B5 hook).
    await startCamClient("c", port, { env: relayenv.hookEnv({ keepOldKey: true }) });
    for (const id of ["b", "c"] as const) await on(id, () => setRoomOpen(true));
  });

  after(async () => {
    await apps.stopB();
    await apps.stopC();
    await relayenv.stopRelay();
  });

  it("AC-8.12: after C leaves, the keys C kept decrypt none of A's new frames", async () => {
    await turnCameraOn();
    await on("b", () => waitTile(NAMES.a));
    await on("c", () => waitTile(NAMES.a));
    await on("c", () => app.invoke("voice_leave"));
    // A rekeys for the remaining viewers; B keeps playing with the new key.
    await sleep(8_000);
    await on("b", () => waitTile(NAMES.a));
    const log = clientLog("c");
    expect(log).not.toContain("old broadcast key decrypted a frame");
    await on("c", joinIfReadOnly);
  });

  it("AC-8.10: leaving voice stops the camera at once and removes the tile", async () => {
    await on("c", () => setRoomOpen(true));
    await on("c", () => waitTile(NAMES.a));
    await app.invoke("voice_leave");
    await waitCameraState("off", STOP_MS);
    await on("b", () => waitNoTile(NAMES.a));
    await on("c", () => waitNoTile(NAMES.a));
    await joinIfReadOnly();
  });

  it("AC-8.10: disconnecting stops the camera at once and removes the tile", async () => {
    await turnCameraOn();
    await on("b", () => waitTile(NAMES.a));
    await app.invoke("disconnect", { serverId: await serverId() });
    await waitCameraState("off", STOP_MS);
    await on("b", () => waitNoTile(NAMES.a));
    await on("c", () => waitNoTile(NAMES.a));
    await reconnect();
    await joinIfReadOnly();
  });

  it("AC-8.10: quitting stops the camera and removes the tile", async () => {
    // C quits with its camera on; A and B watch the tile go.
    await setRoomOpen(true);
    await on("c", turnCameraOn);
    await waitTile(NAMES.c);
    await on("b", () => waitTile(NAMES.c));
    // The app exits while the command runs, so its answer never comes.
    await on("c", () => app.invoke("close_choice", { choice: "quit", remember: false }).catch(() => undefined));
    await waitNoTile(NAMES.c);
    await on("b", () => waitNoTile(NAMES.c));
    await apps.stopC();
  });

  it("AC-8.11: the relay going down turns the camera off with its notice, without a direct attempt", async () => {
    await turnCameraOn();
    await on("b", () => waitTile(NAMES.a));
    await startCapture();
    // Only the relay stops; the Mumble server and the relay bot keep running.
    const project = process.env.MUMPEER_PROJECT ?? "mumpeer-testenv";
    const relayContainer = await runOk("docker", [
      "ps", "-q",
      "--filter", `label=com.docker.compose.project=${project}`,
      "--filter", "label=com.docker.compose.service=mumpeer",
    ]);
    await runOk("docker", ["stop", "-t", "1", relayContainer.stdout.trim()]);
    const view = await waitCameraState("off", 60_000);
    expect(view.error).toBe("relayDown");
    await waitText("Camera stopped: the relay isn't reachable");
    // No pair request for a direct connection went out after the relay was gone.
    const peerRequests = (await captured()).filter(
      (p) => p.direction === "sent" && p.kind === "mumcord.peer" && (p.decoded ?? "").includes("\"pair"),
    );
    expect(peerRequests).toEqual([]);
    expect(await browser.execute(() => document.body.innerText)).not.toContain("Connect directly");
  });
});
