/**
 * The missing-codec hint (FR-2.41a). Every Linux package brings its own H.264/AAC decoders, but a
 * source build has only the system's, and some distributions ship WebKitGTK without them; videos
 * that need H.264 then fail. The hint names the package manager command that installs gst-libav. It is only shown
 * when the player failed in a way a missing decoder causes and the webview really lacks H.264,
 * so a generic failure never gets a misleading install hint.
 */
import type { LinuxDistro } from "../../api";

export type PlaybackFailure =
  /** YouTube's player reported an error code (onError). */
  | { kind: "youtube"; code: number }
  /** A Twitch player; Twitch streams only in H.264 and its embed reports no errors. */
  | { kind: "twitch" };

/** YouTube's "HTML5 player error": the video could not be decoded. */
const YOUTUBE_HTML5_ERROR = 5;

/** H.264 baseline in MP4, the format Twitch and most .mp4 files use. */
export const H264_TYPE = 'video/mp4; codecs="avc1.42E01E"';

const TWITCH_PLAYERS = new Set(["https://player.twitch.tv", "https://clips.twitch.tv"]);

export function isCodecFailure(failure: PlaybackFailure): boolean {
  return failure.kind === "twitch" || failure.code === YOUTUBE_HTML5_ERROR;
}

/** Whether a chat card's player URL is a Twitch player. */
export function isTwitchPlayer(playerUrl: string): boolean {
  try {
    return TWITCH_PLAYERS.has(new URL(playerUrl).origin);
  } catch {
    return false;
  }
}

/**
 * Whether a card's player belongs to the browser instead of the card (issue #419). Twitch's embed
 * only runs when every page above it is http(s), and it refuses the whole chain otherwise, showing
 * nothing at all. Mumcord's own page is `tauri://localhost` on Linux and macOS, so a Twitch card
 * there hands the stream to the browser; on Windows (`http(s)://tauri.localhost`) and under
 * `npm run tauri dev` (http) it plays in the card. YouTube's player has no such check.
 */
export function twitchNeedsBrowser(playerUrl: string, appPageIsWeb: boolean): boolean {
  return !appPageIsWeb && isTwitchPlayer(playerUrl);
}

export interface H264Probe {
  /** `MediaSource.isTypeSupported`, which YouTube and Twitch use; absent without MSE. */
  mse: ((type: string) => boolean) | null;
  /** `HTMLMediaElement.canPlayType`. */
  element: (type: string) => string;
}

function webviewProbe(): H264Probe {
  const video = document.createElement("video");
  return {
    mse: typeof MediaSource === "undefined" ? null : (type) => MediaSource.isTypeSupported(type),
    element: (type) => video.canPlayType(type),
  };
}

/** Whether the webview can decode H.264; either way of asking saying yes counts. */
export function supportsH264(probe: H264Probe = webviewProbe()): boolean {
  return probe.mse?.(H264_TYPE) === true || probe.element(H264_TYPE) !== "";
}

export interface InstallHint {
  /** The command for the distribution's package manager, or `null` for the generic text. */
  command: string | null;
  /** Fedora's gst-libav comes from RPM Fusion, which is not enabled by default. */
  rpmFusion: boolean;
}

const APT = ["debian", "ubuntu"];
const DNF = ["fedora", "rhel", "centos"];
const PACMAN = ["arch"];
const ZYPPER = ["opensuse", "suse", "sles"];

/** The install command for gst-libav, picked from os-release's `ID`, then `ID_LIKE`. */
export function installHint(distro: LinuxDistro | null): InstallHint {
  const ids = distro === null ? [] : [distro.id, ...distro.idLike];
  for (const id of ids) {
    // openSUSE's IDs are e.g. "opensuse-tumbleweed" and "opensuse-leap".
    const is = (names: string[]) => names.some((name) => id === name || id.startsWith(`${name}-`));
    if (is(APT)) return { command: "sudo apt install gstreamer1.0-libav", rpmFusion: false };
    if (is(DNF)) return { command: "sudo dnf install gstreamer1-libav", rpmFusion: true };
    if (is(PACMAN)) return { command: "sudo pacman -S gst-libav", rpmFusion: false };
    if (is(ZYPPER)) {
      return { command: "sudo zypper install gstreamer-plugins-libav", rpmFusion: false };
    }
  }
  return { command: null, rpmFusion: false };
}
