/**
 * Messages between the app and the loopback player pages (FR-2.41a). The app serves them from
 * `http://localhost:<port>/<token>/`: `player.html` hosts the watch party's YouTube player,
 * `embed.html` a YouTube or Twitch player of a chat card, because YouTube refuses to play inside
 * the app's own `tauri://` origin and Twitch needs an https or `localhost` parent. Both sides only
 * accept these typed messages; the page speaks YouTube's own control protocol.
 */
import { isVideoId } from "./youtube";

export type BridgeCommand =
  | { type: "load"; videoId: string }
  | { type: "embed"; url: string }
  | { type: "play" }
  | { type: "pause" }
  | { type: "seek"; seconds: number }
  /**
   * Video volume as a fraction of the viewer's own player volume, 0–1 (auto-ducking, FR-2.76);
   * the page fades to it.
   */
  | { type: "setVolume"; level: number }
  /** The viewer's own volume as a fraction, 0–1 (issue #163); ducking multiplies it. */
  | { type: "setUserVolume"; level: number };

export type BridgeReport =
  | { type: "ready" }
  | { type: "state"; state: number }
  | { type: "time"; seconds: number }
  /** How far the video is and how much of it the player has (issue #163). */
  | { type: "meta"; seconds: number; bufferedSeconds: number | null }
  | { type: "ended" }
  | { type: "error"; code: number };

/**
 * The origin of a loopback page URL as `watch_party_player_url` (`page` "player") or
 * `embed_player_open` (`page` "embed") hands it out, or `null` for anything else.
 */
export function playerOrigin(pageUrl: string, page: "player" | "embed" = "player"): string | null {
  let url: URL;
  try {
    url = new URL(pageUrl);
  } catch {
    return null;
  }
  const loopback = url.protocol === "http:" && url.hostname === "localhost" && url.port !== "";
  const path = new RegExp(`^/[0-9a-f]{32}/${page}\\.html$`);
  return loopback && url.search === "" && path.test(url.pathname) ? url.origin : null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

/** A report from the player page; anything else yields `null`. */
export function parseBridgeReport(data: unknown): BridgeReport | null {
  if (!isRecord(data)) return null;
  switch (data.type) {
    case "ready":
    case "ended":
      return { type: data.type };
    case "state":
      return isNumber(data.state) ? { type: "state", state: data.state } : null;
    case "time":
      return isNumber(data.seconds) ? { type: "time", seconds: data.seconds } : null;
    case "error":
      return { type: "error", code: isNumber(data.code) ? data.code : 0 };
    case "meta":
      return isNumber(data.seconds) && (data.bufferedSeconds === null || isNumber(data.bufferedSeconds))
        ? { type: "meta", seconds: data.seconds, bufferedSeconds: data.bufferedSeconds ?? null }
        : null;
    default:
      return null;
  }
}

/** The load command for a video, or `null` for anything but a YouTube video id. */
export function loadCommand(videoId: string): BridgeCommand | null {
  return isVideoId(videoId) ? { type: "load", videoId } : null;
}

/** The setVolume command for a ducking level, clamped to 0–1; `null` for a non-number. */
export function volumeCommand(level: number): BridgeCommand | null {
  return isNumber(level) ? { type: "setVolume", level: Math.min(1, Math.max(0, level)) } : null;
}

/** The viewer's own volume as a fraction, 0–1 (issue #163); `null` for a non-number. */
export function userVolumeCommand(level: number): BridgeCommand | null {
  return isNumber(level) ? { type: "setUserVolume", level: Math.min(1, Math.max(0, level)) } : null;
}
