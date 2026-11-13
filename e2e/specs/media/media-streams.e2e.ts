// Spec 07 §11, the 0.9.0 "media" stream format on the path that needs no relay: two 0.9.0 clients
// with no relay and no bot watch over a new-format direct `STREAM_SCREEN` share, and a viewer that
// joins mid-share gets the description before its first frame.
//
//   AC-7.80  (no-relay half) B's watch reports the direct path, A's own share reports the direct
//            fan-out ("this server has no relay"), B's window never shows the "relay needs an
//            update" text, and the share keeps running.
//   AC-7.85  three clients: A shares, B watches direct, C joins the channel mid-share and gets the
//            description first; B's keyframe request (`screen_share_request_keyframe`) brings a
//            keyframe on every video channel within 1 s.
//
// The relay cells of the same format live in `media-streams-relay.e2e.ts`: they need a shard that
// starts the relay environment, and such a shard cannot offer the relay-free server these two
// assert. Keeping them apart is what lets both run.
//
// Not automatable in this environment, so covered elsewhere or reported:
//   - AC-7.72's capture on the relay host, AC-7.75's relay-side layer counters, AC-7.77(c/d),
//     AC-7.78 to 7.79 and 7.81 to 7.88 — see the relay spec and the acceptance inventory
//     (`specs/07-08-bovnar-media-streams-0.9.0-acceptance-inventory.md`).
//   - AC-7.85's "no SFrame header" on the direct path: the inspector keeps no payload (FR-7.101),
//     so there is nothing to read.
//
// The share itself is the test backend's pattern source (the app is built with `test-backend`, see
// e2e/container-run.sh), so no real screen is picked; `sharePick: "screen"` is the test hook.
//
// Runs without a mumpeer checkout: nothing here needs the relay.
// Run: npm run e2e -- --spec e2e/specs/media/media-streams.e2e.ts
import { browser, expect } from "@wdio/globals";
import { app, apps, relayenv, testenv } from "../../helpers/index.ts";
import { SHARER, share, startSharer, startViewer, watch } from "../../helpers/share-sound-ui.ts";
import { on, pageText, serverId, sessionOf, startCapture } from "../camera/flows.ts";
import {
  KEYFRAME_MS,
  expectDocumentOnly,
  firstCaptured,
  isDescription,
  listedShares,
  mediaRows,
  startLateViewer,
  videoChannel,
  videoChannels,
  waitFanout,
  waitPath,
  watchOn,
} from "./flows.ts";

describe("media streams, the 0.9.0 share format without a relay (AC-7.80, 7.85)", function () {
  this.timeout(900_000);
  this.bail(true);

  before(async () => {
    // A shard that also carries relay specs keeps the relay environment running between spec
    // files; no relay may run while these cells assert that there is none. No-op when none runs.
    await relayenv.stopRelay();
  });

  after(async () => {
    await apps.stopB();
    await apps.stopC();
  });

  it("AC-7.80: with no relay the pair watches on a direct share, without any relay text", async () => {
    // The plain test server: no relay and no bot, so the direct path is the only one (FR-7.98).
    await testenv.up("v15");
    const plain = testenv.ports.v15;
    await startSharer({ sharePick: "screen" }, plain);
    await startViewer(plain);
    // From before the share, so A's first media message can be the description (FR-7.89).
    await startCapture();
    await share();
    await watch();
    await on("b", () => waitPath("direct"));

    // One `.media` document: the description on channel 0 first, then frames on the layer
    // channels, and never the old header (FR-7.89, FR-7.98).
    const rows = await mediaRows();
    const head = firstCaptured(rows, "A's share");
    expect(isDescription(head)).toBe(true);
    expect(head.media?.channel).toBe(0);
    expectDocumentOnly(rows);
    // The description says what the stream carries: a screen, its timebase and its tracks.
    const description = JSON.parse(head.decoded ?? "{}") as {
      kind?: string;
      timebase?: unknown;
      tracks?: unknown[];
    };
    expect(description.kind).toBe("screen");
    expect(typeof description.timebase).toBe("number");
    expect(description.tracks?.length ?? 0).toBeGreaterThan(0);
    // Frames on 1 + layer, and nothing to restart the document with.
    expect(videoChannels(rows).length).toBeGreaterThan(0);
    expect(rows.slice(1).filter(isDescription)).toHaveLength(0);

    // No relay, so the share sends one copy per viewer and says why (FR-7.47); a relay session
    // cannot exist, because the environment runs no relay at all.
    const mode = await waitFanout();
    expect(mode.kind).toBe("direct");
    expect(mode.reason).toBe("noRelay");
    // Only an old relay makes the client say the relay needs an update (FR-7.98).
    expect(await pageText()).not.toContain("needs an update");
    await on("b", async () => {
      expect(await pageText()).not.toContain("needs an update");
    });

    // The share never stops (AC-7.80): it is still there and B still watches it.
    expect(await listedShares()).toHaveLength(1);
    await on("b", () => waitPath("direct", 5_000));
  });

  it("AC-7.85: C joins mid-share and gets the description first, and a keyframe comes within 1 s", async () => {
    await startLateViewer(testenv.ports.v15);
    await watchOn("c");
    await apps.onC(() => waitPath("direct"));

    // The late joiner's first message is the description, then its frames (FR-7.89, FR-7.97).
    const late = await apps.onC(async () => {
      let seen = await mediaRows();
      await browser
        .waitUntil(
          async () => {
            seen = await mediaRows();
            return videoChannels(seen).length > 0;
          },
          { timeout: 30_000, interval: 250 },
        )
        .catch(() => {
          throw new Error("C's view received no frame");
        });
      return seen;
    });
    expect(isDescription(firstCaptured(late, "C's view"))).toBe(true);
    expectDocumentOnly(late);
    expect(late.filter((row) => videoChannel(row) !== null).length).toBeGreaterThan(0);

    // A keyframe is always temporal layer 0, so it arrives on channel 1. Every layer choice
    // includes that channel (FR-7.90); the enhancement channels are not keyframes of their own.
    await startCapture();
    await browser.waitUntil(
      async () => videoChannels(await mediaRows()).includes(1),
      { timeout: 30_000, interval: 250, timeoutMsg: "A's share sent no base-layer frame" },
    );
    const asked = Date.now();
    await apps.onB(async () => {
      await app.invoke("screen_share_request_keyframe", {
        serverId: await serverId(),
        session: await sessionOf(SHARER),
      });
    });
    let keyframeAt = 0;
    await browser
      .waitUntil(
        async () => {
          for (const row of await mediaRows()) {
            const at = row.atMs ?? 0;
            if (videoChannel(row) !== 1 || row.media?.keyframe !== true || at < asked) continue;
            keyframeAt = at;
            return true;
          }
          return false;
        },
        { timeout: 20_000, interval: 100 },
      )
      .catch(() => {
        throw new Error(`no base-layer keyframe after ${String(KEYFRAME_MS)} ms`);
      });
    expect(keyframeAt - asked).toBeLessThanOrEqual(KEYFRAME_MS);
  });
});
