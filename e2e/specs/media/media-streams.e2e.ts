// Spec 07 §11, the 0.9.0 "media" stream format (FR-7.89 … FR-7.101) in the running client: which
// path a share takes with and without a relay, what its packet inspector holds, and what a viewer
// that joins mid-share gets.
//
//   AC-7.80  (no relay) two 0.9.0 clients with no relay and no bot watch each other on a
//            new-format direct `STREAM_SCREEN` share: B's watch reports the direct path, A's own
//            share reports the direct fan-out ("this server has no relay"), B's window never shows
//            the "relay needs an update" text, and the share keeps running.
//   AC-7.85  no relay, no bot, three clients: A shares, B watches direct, C joins the channel
//            mid-share and gets the description before its first frame; B's keyframe request
//            (`screen_share_request_keyframe`) brings a keyframe on every video channel within 1 s.
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
// Not automatable in this environment, so covered elsewhere or reported:
//   - AC-7.80's old-relay half (profile `old`, "This server's relay needs an update"): an old relay
//     needs the previous mumpeer and bot images (`MUMCORD_E2E_OLD_MUMPEER_IMAGE`, 0.1.0), which
//     carry no media profile at all. Reported as not verified; the no-relay half is automated here.
//   - AC-7.72's capture on the relay host (only channel, flags and the SFrame header, never a
//     timestamp, resolution or frame sequence) and AC-7.85's "no SFrame header" on the direct path:
//     a capture on the relay's own port needs `sudo tcpdump`, and the inspector keeps no payload at
//     all (FR-7.101), so neither can be read from a spec.
//   - AC-7.76's "a test hook holding B's old key cannot decrypt the new frames": the `keepOldKey`
//     hook of `camera/stop.e2e.ts` only tries the keys it kept on old-format units
//     (`crates/engine/src/screen_share/broadcast/link.rs`, `try_old_key`), and the media profile
//     drops what a detached subscriber still gets. Engine test, reported as not verified.
//   - AC-7.74 (a quality change sends a new description, applied at a keyframe): engine tests
//     (`crates/engine/src/screen_share/wire.rs`); a picture decoded with the wrong description has
//     no UI signal of its own.
//   - AC-7.75 (a viewer's layer choice reaches only channels 1–2): the relay drops the layers, and
//     the difference is a frame count in the capture, which is too noisy to read here; engine and
//     relay tests.
//   - AC-7.77(c/d): `(c)` needs `sudo testenv/netns.sh noudp`, `(d)` needs a 0.8.x build.
//   - AC-7.78, 7.79, 7.81, 7.88: need a real 0.8.x client, which the E2E image does not carry.
//   - AC-7.82: allocation counters and the relay benchmark, not a UI signal.
//   - AC-7.83: the C reference and the CI fuzz budget.
//   - AC-7.84: dark and light screenshots and all 24 locales, a manual visual check.
//   - AC-7.86: the relay forced to TCP 443 (`sudo testenv/netns.sh noudp`) and the relay log.
//   - AC-7.87: the relay forwards at most one `kf` per 2 s per subscriber; `/v1/status` exposes no
//     such counter and the relay log is not readable from a spec.
//
// The share itself is the test backend's pattern source (the app is built with `test-backend`, see
// e2e/container-run.sh), so no real screen is picked; `sharePick: "screen"` is the test hook.
//
// The three relay cells run only against a relay that carries the media profile (`/v1/status`
// `"media": 2`); with an older relay they skip rather than test a format it cannot serve. The
// mumpeer commit the E2E workflow pins (MUMPEER_REF) is older than the media profile, so on CI they
// skip until that pin moves.
//
// Skips itself without a mumpeer checkout (see relay-uses.e2e.ts); the sound cell also skips
// itself without PipeWire.
// Run: npm run e2e -- --spec e2e/specs/media/media-streams.e2e.ts
import { browser, $, expect } from "@wdio/globals";
import { app, apps, pwapps, relayenv, testenv } from "../../helpers/index.ts";
import type { AppId } from "../../helpers/app.ts";
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
import { addAndConnect, completeWizard, joinIfReadOnly } from "../../helpers/voice-ui.ts";
import {
  clientLog,
  on,
  pageText,
  serverId,
  sessionOf,
  startCapture,
  updateSettings,
} from "../camera/flows.ts";

const P1 = "Test App One";
const P2 = "Test App Two";

/** C joins mid-share, as AC-7.85 and AC-7.73 ask (names of `share-sound-ui.ts` for A and B). */
const LATE = "Carol";

/** A keyframe request must be answered within 1 s (AC-7.85, FR-7.96). */
const KEYFRAME_MS = 1_000;

/** How the own share reaches its viewers (FR-7.47). */
type Fanout = { kind: string; reason: string | null };

/** What the current client's watch of A's share looks like (issue #163); `null` while not
 * watching. */
interface WatchStats {
  path: "direct" | "broadcast";
  rms: number | null;
}

/** One media message of a captured stream (FR-7.101): never the payload, only its shape. */
interface MediaRow {
  atMs: number | null;
  /** The decoded description of a channel-0 description message, `null` for everything else. */
  decoded: string | null;
  media: {
    format: "media" | "legacy";
    channel: number | null;
    control: number | null;
    flags: number | null;
    keyframe: boolean;
  } | null;
}

/** A share as `screen_share_list` reports it. */
interface ListedShare {
  session: number;
  paused: boolean;
  viewers: number[];
}

/** The own share's watch of A's share, or `null` when it does not watch it. */
async function watchStats(): Promise<WatchStats | null> {
  return app.invoke<WatchStats | null>("screen_share_watch_stats", {
    serverId: await serverId(),
    session: await sessionOf(SHARER),
  });
}

/** Waits until the watch of A's share takes `path`; the stream and its keys take a moment. */
async function waitPath(path: WatchStats["path"], timeout = 45_000): Promise<void> {
  let seen: string | undefined;
  await browser
    .waitUntil(
      async () => {
        seen = (await watchStats())?.path;
        return seen === path;
      },
      { timeout, interval: 500 },
    )
    .catch(() => {
      throw new Error(`the watch goes via ${String(seen)}, want ${path}`);
    });
}

/** The shares in the current client's channel. */
async function listedShares(): Promise<ListedShare[]> {
  return app.invoke<ListedShare[]>("screen_share_list", { serverId: await serverId() });
}

/** How the own share reaches its viewers; `null` while it has not asked for fan-out yet. */
async function fanout(): Promise<Fanout | null> {
  const quality = await app.invoke<{ fanout: (Fanout & { limit?: number }) | null } | null>(
    "screen_share_quality",
    { serverId: await serverId() },
  );
  const own = quality?.fanout ?? null;
  return own === null ? null : { kind: own.kind, reason: own.reason };
}

/** Waits until the own share reports how it reaches its viewers (FR-7.47). */
async function waitFanout(timeout = 30_000): Promise<Fanout> {
  const started = Date.now();
  for (;;) {
    const mode = await fanout();
    if (mode !== null && mode.kind !== "starting") return mode;
    if (Date.now() - started > timeout) throw new Error("A's share reports no fan-out");
    await browser.pause(500);
  }
}

/** The media messages the current client's packet inspector holds (`null`: every kept row). */
async function mediaRows(): Promise<MediaRow[]> {
  const { packets } = await app.invoke<{ packets: MediaRow[] }>("packet_capture_poll", {
    after: null,
  });
  return packets.filter((row) => row.media !== null);
}

/** The video layer channel (1–4) of a captured message, or `null` for the control and audio ones. */
function videoChannel(row: MediaRow): number | null {
  const media = row.media;
  if (media === null || media.control !== null) return null;
  const channel = media.channel;
  return channel !== null && channel >= 1 && channel <= 4 ? channel : null;
}

/** The video layer channels among captured messages, in order. */
function videoChannels(rows: MediaRow[]): number[] {
  const seen = new Set<number>();
  for (const row of rows) {
    const channel = videoChannel(row);
    if (channel !== null) seen.add(channel);
  }
  return [...seen].sort((a, b) => a - b);
}

/** Whether the row is the channel-0 description (the document itself, not a frame). */
function isDescription(row: MediaRow): boolean {
  return row.media?.control === 0;
}

/** The first captured message; throws when the capture holds none. */
function firstCaptured(rows: MediaRow[], what: string): MediaRow {
  const row = rows.at(0);
  if (row === undefined) throw new Error(`${what} captured no media message`);
  return row;
}

/** Every captured message is the 0.9.0 document, never the 0.8.x header (FR-7.89, FR-7.98). */
function expectDocumentOnly(rows: MediaRow[]): void {
  for (const row of rows) expect(row.media?.format).toBe("media");
}

/** The The Watch button under A's share banner on `id`, until its view has connected. The helper
 * `watch` of the share-sound specs is written for B; C gets the same two steps here. */
async function watchOn(id: AppId): Promise<void> {
  await on(id, async () => {
    const button = $(
      `//div[contains(@class,'activity-banner')][contains(.,'${SHARER} is sharing their screen')]//button[normalize-space()='Watch']`,
    );
    await button.waitForClickable({
      timeout: 30_000,
      timeoutMsg: `${SHARER}'s share never showed`,
    });
    await button.click();
    const connecting = $(`//*[contains(text(),"Connecting to ${SHARER}'s screen")]`);
    await browser.waitUntil(async () => !(await connecting.isExisting()), {
      timeout: 45_000,
      timeoutMsg: `the view of ${SHARER}'s share never connected`,
    });
  });
}

/**
 * C enters the channel of A's share, with its packet inspector capturing from before its watch so
 * its first message is the description (FR-7.89).
 */
async function startLateViewer(port: number): Promise<void> {
  await apps.stopC();
  await apps.startC();
  await on("c", async () => {
    await completeWizard(LATE);
    // Without that answer the first watch asks both people first; not what this spec is about.
    await updateSettings({ allowDirectConnections: "yes" });
    await addAndConnect(`localhost:${String(port)}`);
    await joinIfReadOnly();
    await browser.setWindowSize(1400, 900);
    await startCapture();
  });
}

/** The frame rate the current client counts from the frames it receives (issue #163). */
async function viewerFps(): Promise<number> {
  const match = /(\d+)\s*fps/.exec(await pageText());
  return match === null ? 0 : Number(match[1]);
}

(relayenv.relayAvailable() ? describe : describe.skip)(
  "media streams, the 0.9.0 share format (AC-7.72, 7.73, 7.76, 7.80, 7.85)",
  function () {
    this.timeout(900_000);
    this.bail(true);

    /** The relay environment's server, once AC-7.72 has started it. */
    let port = 0;
    /** Whether C already runs on the relay environment, watching A's relay share. */
    let lateOnRelay = false;

    after(async () => {
      pwapps.stopTestApps();
      await apps.stopB();
      await apps.stopC();
      cleanUpSound();
      await relayenv.stopRelay();
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

      // B's keyframe request (as after a lost frame) makes A send a keyframe within 1 s on every
      // video channel it is using (FR-7.96); the channels are read off A's own frames first.
      await startCapture();
      let channels: number[] = [];
      await browser.waitUntil(
        async () => {
          channels = videoChannels(await mediaRows());
          return channels.length > 0;
        },
        { timeout: 30_000, interval: 250, timeoutMsg: "A's share sent no video frame" },
      );
      const asked = Date.now();
      await apps.onB(async () => {
        await app.invoke("screen_share_request_keyframe", {
          serverId: await serverId(),
          session: await sessionOf(SHARER),
        });
      });
      const keyframeAt = new Map<number, number>();
      await browser
        .waitUntil(
          async () => {
            for (const row of await mediaRows()) {
              const channel = videoChannel(row);
              const at = row.atMs ?? 0;
              if (channel === null || row.media?.keyframe !== true || at < asked) continue;
              if (!keyframeAt.has(channel)) keyframeAt.set(channel, at);
            }
            return channels.every((channel) => keyframeAt.has(channel));
          },
          { timeout: 20_000, interval: 100 },
        )
        .catch(() => {
          const missing = channels.filter((channel) => !keyframeAt.has(channel));
          throw new Error(
            `no keyframe after ${String(KEYFRAME_MS)} ms on channel(s) ${missing.join(", ")}`,
          );
        });
      for (const channel of channels) {
        expect((keyframeAt.get(channel) ?? Infinity) - asked).toBeLessThanOrEqual(KEYFRAME_MS);
      }
    });

    describe("the new relay's media profile", function () {
      before(async function () {
        this.timeout(20 * 60_000);
        ({ port } = await relayenv.startRelay("all"));
        // The 0.9.0 share format needs a relay with the media profile (/v1/status `"media": 2`);
        // an older relay serves the old broadcast, which the cells below cannot read.
        const { media } = (await relayenv.relayStatus()) as { media?: number };
        if (media !== 2) this.skip();
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
    });
  },
);
