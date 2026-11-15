// Spec 08 §4.14, the camera path matrix on the path that needs no relay: two 0.9.0 clients with no
// relay and no bot watch each other over the direct camera stream, and a client that enters the
// channel later gets its own stream.
//
//   AC-8.29  the tile arrives within 3 s, the room header says "Direct · end-to-end encrypted", the
//            viewer list says Direct, and C entering the channel mid-camera gets its own stream.
//
// The relay cells of the same matrix live in `camera/media-streams.e2e.ts` (AC-8.30 to 8.33). They
// need a shard that starts the relay environment, and such a shard cannot offer the relay-free
// server this cell asserts — with a relay running the room correctly names it, and the cell fails.
// Keeping the two apart is what lets both run.
//
// Runs without a mumpeer checkout: nothing here needs the relay.
// Run: npm run e2e -- --spec e2e/specs/camera/media-streams-direct.e2e.ts
import { $, $$, expect } from "@wdio/globals";
import { apps, relayenv, testenv } from "../../helpers/index.ts";
import {
  NAMES,
  on,
  sessionOf,
  setRoomOpen,
  startCamClient,
  turnCameraOn,
  waitOwnViewer,
  waitTile,
} from "./flows.ts";

/** How the room names the own camera's path (camera.json `room.*`, FR-8.62). */
const DIRECT_CHIP = "Direct · end-to-end encrypted";

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

describe("camera direct path, no relay (AC-8.29)", function () {
  this.timeout(900_000);
  this.bail(true);

  let bob = 0;
  let carol = 0;

  before(async () => {
    // A shard that also carries relay specs keeps the relay environment running between spec
    // files; none may run while this cell asserts the direct path is the only one. No-op when the
    // environment is down or no mumpeer checkout exists.
    await relayenv.stopRelay();
  });

  after(async () => {
    await apps.stopB();
    await apps.stopC();
  });

  it("AC-8.29: with no relay the two 0.9.0 clients watch each other over the direct path", async () => {
    // The plain test server: no relay, no bot (as in avail.e2e.ts), so the direct path is the
    // only one (FR-8.96).
    await testenv.up("v15");
    const plain = testenv.ports.v15;
    await startCamClient("a", plain);
    await startCamClient("b", plain);
    for (const id of ["a", "b"] as const) await on(id, () => setRoomOpen(true));
    bob = await sessionOf(NAMES.b);

    const view = await turnCameraOn();
    // FR-8.94: no usable relay, so no relay broadcast is created at all.
    expect(view.broadcast).toBeNull();
    // The tile arrives within 3 s (FR-8.60) over A's own `STREAM_CAMERA` (FR-8.92).
    await on("b", () => waitTile(NAMES.a, "playing", TILE_MS));
    expect((await waitOwnViewer(bob)).path).toBe("direct");
    // The room names the path: header chip and viewer list (FR-8.62).
    expect(await pathChip().getText()).toBe(DIRECT_CHIP);
    expect(await viewerListPaths()).toEqual(["Direct"]);

    // C enters the channel while the camera is on: its own stream opens, its tile arrives.
    await startCamClient("c", plain);
    await on("c", async () => {
      await setRoomOpen(true);
      await waitTile(NAMES.a, "playing", TILE_MS);
    });
    carol = await sessionOf(NAMES.c);
    expect((await waitOwnViewer(carol)).path).toBe("direct");
    expect(await viewerListPaths()).toEqual(["Direct", "Direct"]);
  });
});
