// Spec 07 §11, the 0.9.0 "media" stream format over the relay: B watches A's share through the
// relay broadcast, the inspector shows one endless `.media` document, the sound travels on channel
// 5, and a viewer that stops watching lets A rekey without interrupting the others.
//
//   AC-7.72  (relay half) on the new relay (profile `all`, media profile 2) B watches through the
//            relay broadcast, and A's inspector holds one endless `.media` document: never the old
//            header, never a description inside a running window, frames on the layer channels.
//   AC-7.73  (relay half) the same relay share with desktop sound: B hears it, A's inspector shows
//            the audio messages on channel 5, and C joining mid-share gets the description before
//            anything else and hears the share (its watch's sound level rises; the sound helper
//            records one viewer's output, so B's recorded tones are the sound proof).
//   AC-7.76  (visible half) on the new relay B stops watching, A rekeys for the remaining viewer,
//            and C keeps watching without an interruption in its picture.
//
// The no-relay cells of the same format live in `media-streams.e2e.ts`: a shard that starts this
// relay environment cannot offer the relay-free server those two assert, so the two files are kept
// apart and scheduled into different shards (scripts/e2e-shards.js).
//
// Not automatable in this environment, so covered elsewhere or reported:
//   - AC-7.72's capture on the relay host (only channel, flags and the SFrame header): a capture on
//     the relay's own port needs `sudo tcpdump`, and the inspector keeps no payload at all
//     (FR-7.101), so it cannot be read from a spec.
//   - AC-7.76's "a test hook holding B's old key cannot decrypt the new frames": the `keepOldKey`
//     hook of `camera/stop.e2e.ts` only tries the keys it kept on old-format units
//     (`crates/engine/src/screen_share/broadcast/link.rs`, `try_old_key`). Engine test.
//   - AC-7.74, 7.75, 7.77(c/d), 7.78, 7.79, 7.81 to 7.84, 7.86, 7.87, 7.88 — see the acceptance
//     inventory: engine tests, a real 0.8.x build, `sudo testenv/netns.sh`, or the relay's own log.
//
// The three cells run only against a relay that carries the media profile (`/v1/status`
// `"media": 2`); with an older relay they skip rather than test a format it cannot serve. The
// mumpeer commit the E2E workflow pins (MUMPEER_REF) is older than the media profile, so on CI they
// skip until that pin moves.
//
// Skips itself without a mumpeer checkout (see relay-uses.e2e.ts); the sound cell also skips itself
// without PipeWire.
// Run: npm run e2e -- --spec e2e/specs/media/media-streams-relay.e2e.ts
import { browser, $, expect } from "@wdio/globals";
import { apps, pwapps, relayenv } from "../../helpers/index.ts";
import { registerOnServer } from "../../helpers/files-ui.ts";
import {
  P1_HZ,
  P2_HZ,
  SHARER,
  VIEWER,
  cleanUpSound,
  expectHeard,
  expectSoundLine,
  share,
  startSharer,
  startViewer,
  watch,
} from "../../helpers/share-sound-ui.ts";
import { clientLog, sessionOf, startCapture, updateSettings } from "../camera/flows.ts";
import {
  type MediaRow,
  LATE,
  expectDocumentOnly,
  firstCaptured,
  isDescription,
  listedShares,
  mediaRows,
  startLateViewer,
  videoChannel,
  videoChannels,
  viewerFps,
  waitPath,
  watchOn,
  watchStats,
} from "./flows.ts";

const P1 = "Test App One";
const P2 = "Test App Two";

(relayenv.relayAvailable() ? describe : describe.skip)(
  "media streams over the relay, the 0.9.0 share format (AC-7.72, 7.73, 7.76)",
  function () {
    this.timeout(900_000);
    this.bail(true);

    /** The relay environment's server, once AC-7.72 has started it. */
    let port = 0;
    /** Whether C already runs on the relay environment, watching A's relay share. */
    let lateOnRelay = false;

    before(async function () {
      this.timeout(20 * 60_000);
      ({ port } = await relayenv.startRelay("all"));
      // The 0.9.0 share format needs a relay with the media profile (/v1/status `"media": 2`);
      // an older relay serves the old broadcast, which the cells below cannot read.
      const { media } = (await relayenv.relayStatus()) as { media?: number };
      if (media !== 2) this.skip();
    });

    after(async () => {
      pwapps.stopTestApps();
      await apps.stopB();
      await apps.stopC();
      cleanUpSound();
      await relayenv.stopRelay();
    });

    it("AC-7.72: on the new relay B watches the broadcast and A's inspector holds one `.media` document", async () => {
      await startSharer({ sharePick: "screen" }, port);
      // The relay bot hands out relay tokens to registered users only.
      await registerOnServer();
      // Relay only, so the share goes out as a broadcast instead of one copy per viewer.
      await updateSettings({ p2pMode: "relayOnly" });
      await apps.stopB();
      await startViewer(port);
      await apps.onB(async () => {
        await registerOnServer();
        await updateSettings({ p2pMode: "relayOnly" });
      });
      await share();
      await watch();
      await apps.onB(() => waitPath("broadcast"));

      // A running share: the document is not restarted, so a window of A's inspector holds frames
      // on the layer channels and no description at all (FR-7.89). The first window lets B's key
      // exchange and the description it brings settle first.
      await startCapture();
      let warm: MediaRow[] = [];
      await browser.waitUntil(
        async () => {
          warm = await mediaRows();
          return warm.filter((row) => videoChannel(row) !== null).length > 20;
        },
        { timeout: 30_000, interval: 500, timeoutMsg: "A's broadcast sent no frames" },
      );
      await browser.pause(2_000);
      await startCapture();
      let rows: MediaRow[] = [];
      await browser
        .waitUntil(
          async () => {
            rows = await mediaRows();
            return rows.filter((row) => videoChannel(row) !== null).length > 20;
          },
          { timeout: 30_000, interval: 500 },
        )
        .catch(() => {
          throw new Error(`A's broadcast sent ${String(rows.length)} media messages`);
        });
      expectDocumentOnly(rows);
      expect(rows.filter(isDescription)).toHaveLength(0);
      expect(videoChannels(rows).length).toBeGreaterThan(0);
      // Frames sit on 1 + layer, the sound on channel 5 (FR-7.91); nothing else travels.
      for (const row of rows) {
        const channel = row.media?.channel;
        expect(channel !== null && channel !== undefined && channel >= 1 && channel <= 5).toBe(
          true,
        );
      }
      expect(await listedShares()).toHaveLength(1);
    });

    (pwapps.pipewireRunning() ? it : it.skip)(
      "AC-7.73: the relay share carries the sound, and C joining mid-share gets picture and sound",
      async () => {
        pwapps.startTestApp(P1, P1_HZ);
        pwapps.startTestApp(P2, P2_HZ);
        // The running share carries the desktop sound; the test apps joined it (AC-7.64).
        await expectSoundLine("Sound: desktop");
        await apps.onB(() => expectHeard([P1_HZ, P2_HZ]));
        // The sound travels as audio messages on channel 5 (FR-7.91). Read at A, which sends
        // them: a broadcast viewer's inspector keeps only the control messages it receives.
        await startCapture();
        await browser.waitUntil(
          async () => (await mediaRows()).some((row) => row.media?.channel === 5),
          {
            timeout: 30_000,
            interval: 250,
            timeoutMsg: "A's inspector shows no audio message on channel 5",
          },
        );

        // C enters the channel while the share runs and gets the description before its first
        // frame, then the frames and the sound of the same broadcast (FR-7.89, FR-7.91).
        await startLateViewer(port);
        lateOnRelay = true;
        await apps.onC(() => registerOnServer());
        await watchOn("c");
        await apps.onC(() => waitPath("broadcast"));
        const late = await apps.onC(async () => {
          let seen = await mediaRows();
          await browser.waitUntil(
            async () => {
              seen = await mediaRows();
              return seen.length > 0;
            },
            { timeout: 30_000, interval: 250 },
          );
          return seen;
        });
        const first = firstCaptured(late, "C's view");
        expect(isDescription(first)).toBe(true);
        // ... and it is audible to C: the engine's own sound level of the watch (FR-7.85).
        await apps.onC(async () => {
          let level: number | null = null;
          await browser
            .waitUntil(
              async () => {
                level = (await watchStats())?.rms ?? null;
                return (level ?? 0) > 0;
              },
              { timeout: 20_000, interval: 500 },
            )
            .catch(() => {
              throw new Error(`C's share sound level stays ${String(level)}`);
            });
        });
      },
    );

    it("AC-7.76: B stops watching, A rekeys, and C keeps watching without an interruption", async () => {
      if (!lateOnRelay) {
        await startLateViewer(port);
        lateOnRelay = true;
        await apps.onC(() => registerOnServer());
        await watchOn("c");
        await apps.onC(() => waitPath("broadcast"));
      }
      const alice = await sessionOf(SHARER);
      const bobSession = await sessionOf(VIEWER);
      const carol = await sessionOf(LATE);
      // A rekeys once the relay reports the subscriber gone (FR-7.94); A logs it at info level.
      const rekeys = (): number => (clientLog("a").match(/broadcast rekeyed/g) ?? []).length;
      const before = rekeys();
      // C's own view counts the frames it receives, so a stall shows as 0 fps (issue #163).
      await apps.onC(async () => {
        await browser.waitUntil(async () => (await viewerFps()) > 0, {
          timeout: 30_000,
          timeoutMsg: "C's view counts no frames",
        });
      });

      await apps.onB(async () => {
        const stop = $("button.share-stop");
        await stop.waitForClickable({ timeout: 20_000 });
        await stop.click();
        await browser.waitUntil(async () => (await watchStats()) === null, {
          timeout: 20_000,
          timeoutMsg: "B still watches A's share",
        });
      });

      // A drops B from its viewers and rekeys for the one that is left (FR-7.94).
      await browser
        .waitUntil(
          async () => {
            const own = (await listedShares()).find((s) => s.session === alice);
            return own !== undefined && !own.viewers.includes(bobSession);
          },
          { timeout: 30_000, interval: 500 },
        )
        .catch(() => {
          throw new Error("A still lists B as a viewer");
        });
      await browser.waitUntil(() => Promise.resolve(rekeys() > before), {
        timeout: 30_000,
        timeoutMsg: "A did not rekey after B stopped watching",
      });

      // C keeps watching: its watch is still the broadcast, A still counts it as a viewer, and its
      // picture never drops to 0 frames across the rekey (a black frame has no signal of its own).
      await apps.onC(() => waitPath("broadcast", 5_000));
      const own = (await listedShares()).find((s) => s.session === alice);
      expect(own?.viewers).toContain(carol);
      const until = Date.now() + 3_000;
      while (Date.now() < until) {
        await apps.onC(async () => {
          expect(await viewerFps()).toBeGreaterThan(0);
        });
        await browser.pause(250);
      }
      // A's share keeps running for the remaining viewer.
      expect(await listedShares()).toHaveLength(1);
      expect(own?.paused).toBe(false);
    });
  },
);
