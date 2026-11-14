// Shared read helpers of the 0.9.0 share-format specs: what a watch reports, what the packet
// inspector holds, and the two steps a late viewer takes. Split out so the no-relay spec
// (`media-streams.e2e.ts`) and the relay one (`media-streams-relay.e2e.ts`) stay readable and run
// in different shards — a shard that starts the relay environment cannot offer a server without a
// relay, which the no-relay cell asserts.
import { $, browser, expect } from "@wdio/globals";
import { app, apps } from "../../helpers/index.ts";
import type { AppId } from "../../helpers/app.ts";
import { SHARER } from "../../helpers/share-sound-ui.ts";
import { addAndConnect, completeWizard, joinIfReadOnly } from "../../helpers/voice-ui.ts";
import {
  on,
  pageText,
  serverId,
  sessionOf,
  startCapture,
  updateSettings,
} from "../camera/flows.ts";

/** C joins mid-share, as AC-7.85 and AC-7.73 ask (names of `share-sound-ui.ts` for A and B). */
export const LATE = "Carol";

/** A keyframe request must be answered within 1 s (AC-7.85, FR-7.96). */
export const KEYFRAME_MS = 1_000;

/** How the own share reaches its viewers (FR-7.47). */
export type Fanout = { kind: string; reason: string | null };

/** What the current client's watch of A's share looks like (issue #163); `null` while not
 * watching. */
export interface WatchStats {
  path: "direct" | "broadcast";
  rms: number | null;
}

/** One media message of a captured stream (FR-7.101): never the payload, only its shape. */
export interface MediaRow {
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
export interface ListedShare {
  session: number;
  paused: boolean;
  viewers: number[];
}

/** The own share's watch of A's share, or `null` when it does not watch it. */
export async function watchStats(): Promise<WatchStats | null> {
  return app.invoke<WatchStats | null>("screen_share_watch_stats", {
    serverId: await serverId(),
    session: await sessionOf(SHARER),
  });
}

/** Waits until the watch of A's share takes `path`; the stream and its keys take a moment. */
export async function waitPath(path: WatchStats["path"], timeout = 45_000): Promise<void> {
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
export async function listedShares(): Promise<ListedShare[]> {
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
export async function waitFanout(timeout = 30_000): Promise<Fanout> {
  const started = Date.now();
  for (;;) {
    const mode = await fanout();
    if (mode !== null && mode.kind !== "starting") return mode;
    if (Date.now() - started > timeout) throw new Error("A's share reports no fan-out");
    await browser.pause(500);
  }
}

/** The media messages the current client's packet inspector holds (`null`: every kept row). */
export async function mediaRows(): Promise<MediaRow[]> {
  const { packets } = await app.invoke<{ packets: MediaRow[] }>("packet_capture_poll", {
    after: null,
  });
  return packets.filter((row) => row.media !== null);
}

/** The video layer channel (1–4) of a captured message, or `null` for the control and audio ones. */
export function videoChannel(row: MediaRow): number | null {
  const media = row.media;
  if (media === null || media.control !== null) return null;
  const channel = media.channel;
  return channel !== null && channel >= 1 && channel <= 4 ? channel : null;
}

/** The video layer channels among captured messages, in order. */
export function videoChannels(rows: MediaRow[]): number[] {
  const seen = new Set<number>();
  for (const row of rows) {
    const channel = videoChannel(row);
    if (channel !== null) seen.add(channel);
  }
  return [...seen].sort((a, b) => a - b);
}

/** Whether the row is the channel-0 description (the document itself, not a frame). */
export function isDescription(row: MediaRow): boolean {
  return row.media?.control === 0;
}

/** The first captured message; throws when the capture holds none. */
export function firstCaptured(rows: MediaRow[], what: string): MediaRow {
  const row = rows.at(0);
  if (row === undefined) throw new Error(`${what} captured no media message`);
  return row;
}

/** Waits until the inspector holds a media message. The first packet follows the watch. */
export async function waitMediaRows(what: string, timeout = 30_000): Promise<MediaRow[]> {
  let rows: MediaRow[] = [];
  await browser
    .waitUntil(
      async () => {
        rows = await mediaRows();
        return rows.length > 0;
      },
      { timeout, interval: 250 },
    )
    .catch(() => {
      throw new Error(`${what} captured no media message`);
    });
  return rows;
}

/** Every captured message is the 0.9.0 document, never the 0.8.x header (FR-7.89, FR-7.98). */
export function expectDocumentOnly(rows: MediaRow[]): void {
  for (const row of rows) expect(row.media?.format).toBe("media");
}

/** The Watch button under A's share banner on `id`, until its view has connected. The helper
 * `watch` of the share-sound specs is written for B; C gets the same two steps here. */
export async function watchOn(id: AppId): Promise<void> {
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
export async function startLateViewer(port: number): Promise<void> {
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
export async function viewerFps(): Promise<number> {
  const match = /(\d+)\s*fps/.exec(await pageText());
  return match === null ? 0 : Number(match[1]);
}
