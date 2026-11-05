// Spec 08, camera room size and blocking (AC-8.8 in part, AC-8.13) in the running client against
// the mumpeer test environment (profile `all`). A = Alice, B = Bob and C = Carol each run the test
// camera (MUMCORD_TEST_CAMERA) and are registered (relay tokens need a registration).
//
//   AC-8.8  (part) with 2 cameras the grid is 2 columns × 1 row (mockup 05), with 3 cameras
//           2 × 2, on every viewer, and each sees every camera's tile.
//   AC-8.13 B blocks A → B shows no tile and no tree camera icon for A, and A's camera never
//           counts B as a viewer (A never admits B), while C still sees A.
//
// Manual / not on CI yet:
// - AC-8.7 (channel change): the relay testenv's Mumble server has only the root channel and the
//   E2E users can't create one, so "C moves to another channel" can't be scripted; the scripted
//   cam-join, the relay's session list and the bot's `not-same-channel` refusal also need the relay
//   and bot logs.
// - AC-8.8 with 9 cameras ("9 of 9 cameras", B's "The camera room is full (9 of 9 cameras).") and
//   the 4-camera grid (mockup 06) need the `bcast-sim` camera publishers (FR-8.84, task MS-1),
//   which don't exist yet.
// - AC-8.13 "B sends no cam-join to A (inspector)" and "bot log shows no admit for B": the
//   engine's viewer list of A stands in for both.
//
// Skips itself without a mumpeer checkout.
import { browser, expect } from "@wdio/globals";
import { apps, relayenv } from "../../helpers/index.ts";
import * as camera from "../../helpers/camera.ts";
import { personRow } from "../../helpers/voice-ui.ts";

type On = <T>(fn: () => Promise<T>) => Promise<T>;
const onA: On = (fn) => fn();

/** Waits until the client's camera room shows `tiles` tiles in `cols` columns and each tile named in `names`. */
async function expectGrid(on: On, cols: number, tiles: number, names: string[]): Promise<void> {
  await on(async () => {
    await camera.openRoom();
    let seen: Awaited<ReturnType<typeof camera.gridShape>> = null;
    await browser.waitUntil(
      async () => {
        seen = await camera.gridShape();
        return seen?.cols === cols && seen.tiles === tiles;
      },
      { timeout: 20_000, timeoutMsg: `grid: want ${String(cols)} columns / ${String(tiles)} tiles, got ${JSON.stringify(seen)}` },
    );
    for (const name of names) await camera.waitTilePicture(name);
  });
}

(relayenv.relayAvailable() ? describe : describe.skip)("camera room size and blocking (spec 08, AC-8.8, 8.13)", function () {
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
    for (const on of [onA, apps.onB, apps.onC]) await on(() => camera.expectCameraBlock(null));
  });

  it("AC-8.8: two cameras fill a 2 × 1 grid", async () => {
    await camera.turnOn();
    await apps.onB(() => camera.turnOn());
    await expectGrid(onA, 2, 2, ["Bob"]);
    await expectGrid(apps.onB, 2, 2, ["Alice"]);
    // C has no camera on: two tiles of others.
    await expectGrid(apps.onC, 2, 2, ["Alice", "Bob"]);
  });

  it("AC-8.8: three cameras make a 2 × 2 grid", async () => {
    await apps.onC(() => camera.turnOn());
    await expectGrid(onA, 2, 3, ["Bob", "Carol"]);
    await expectGrid(apps.onB, 2, 3, ["Alice", "Carol"]);
    await expectGrid(apps.onC, 2, 3, ["Alice", "Bob"]);
    for (const on of [onA, apps.onB, apps.onC]) await on(() => camera.turnOff());
  });

  it("AC-8.13: after B blocks A, B gets no tile or icon for A and A never admits B", async () => {
    await apps.onB(() => camera.blockPerson("Alice"));
    await camera.turnOn();
    await apps.onC(async () => {
      await camera.openRoom();
      await camera.waitTilePicture("Alice");
    });
    // C is admitted; B must never be, so watch A's viewers for a while.
    const deadline = Date.now() + 10_000;
    while (Date.now() < deadline) {
      const view = await camera.cameraView();
      expect(view?.state).toBe("onAir");
      expect(view?.viewers.length).toBeLessThanOrEqual(1);
      await browser.pause(500);
    }
    expect((await camera.cameraView())?.viewers).toHaveLength(1);
    await apps.onB(async () => {
      expect(await camera.hasTreeCamera("Alice")).toBe(false);
      expect(await camera.tile("Alice").isExisting()).toBe(false);
    });
    await camera.turnOff();
  });
});
