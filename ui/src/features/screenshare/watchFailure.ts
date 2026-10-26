/**
 * Why watching a share failed, as the viewer explains it (FR-2.43, FR-2.46). The engine reports
 * `directUnavailable` with a `WatchBlock` code as message; everything that never got a picture
 * in time counts as unreachable, so "Connecting…" never stays forever.
 */
import type { EngineError } from "../../api";

export type WatchFailure =
  /** No direct path to the sharer, no answer from them, or no picture in time. */
  | "unreachable"
  /** The sharer's Mumcord cannot take direct connections. */
  | "tooOld"
  /** The user turned direct connections off. */
  | "consentOff"
  /** The sharer already has the most viewers. */
  | "full"
  /** Anything else; the engine's message is logged. */
  | "failed";

/** How long "Connecting…" may last before the viewer gives up. */
export const CONNECT_TIMEOUT_MS = 15_000;

export function watchFailure(error: EngineError): WatchFailure {
  switch (error.code) {
    case "shareFull":
      return "full";
    case "directUnavailable":
      if (error.message === "peerUnsupported") return "tooOld";
      if (error.message === "consentOff") return "consentOff";
      return "unreachable";
    // The sharer's client did not answer the watch request in time. The other `notFound`, an
    // ended share, closes the viewer anyway.
    case "notFound":
      return "unreachable";
    default:
      return "failed";
  }
}
