/**
 * What the own share's sound carries, or why it has none (FR-7.85): the sound line's tooltip and
 * the reason the Sound button is disabled.
 */
import type { TFunction } from "i18next";
import type { ScreenShare, ShareSoundReason } from "../../../api";

const REASON_KEY: Record<Exclude<ShareSoundReason, "osTooOld">, string> = {
  off: "liveBar.tipOff",
  notChosen: "liveBar.tipNone",
  noPipeWire: "liveBar.tipNoPipeWire",
  captureFailed: "liveBar.tipCaptureFailed",
};

/** The own share sends sound right now. */
export function sendsSound(own: ScreenShare): boolean {
  return own.sound && own.soundMode !== "none";
}

/**
 * What the share's sound carries, or why it has none. Without a reason the share has no single
 * app's sound to offer (a window without one app behind it), which reads like "No sound" chosen.
 */
export function soundTip(own: ScreenShare, os: string, t: TFunction<"screenshare">): string {
  if (sendsSound(own)) return t(own.soundMode === "app" ? "liveBar.tipApp" : "liveBar.tipDesktop");
  if (own.soundReason === "osTooOld") {
    return t(
      os.toLowerCase().startsWith("windows") ? "liveBar.tipOldWindows" : "liveBar.tipOldMacos",
    );
  }
  return t(REASON_KEY[own.soundReason ?? "notChosen"]);
}
