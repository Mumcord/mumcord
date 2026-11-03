/** Presentation of an attachment-tray chip: status line text and whether it can be sent. */
import type { TFunction } from "i18next";
import type { ChipError, FileChip } from "../../../bindings/commands";
import { formatSize } from "../../chat/audioPlayer";

/** At most this many files go into one message (FR-7.11). */
export const MAX_CHIPS = 10;

export function chipErrorText(t: TFunction<"files">, error: ChipError): string {
  switch (error.kind) {
    case "tooLarge":
      return t("composer.tooLarge", { name: error.name });
    case "unreadable":
      return t("composer.unreadable", { path: error.path });
    case "serverRefused":
      return t("composer.serverRefused", { reason: error.reason });
    case "other":
      return error.message;
  }
}

function percent(done: number, total: number): number {
  return total > 0 ? Math.min(100, Math.floor((done / total) * 100)) : 0;
}

/** The second line of a chip: size, progress or the reason it failed. */
export function chipStatus(t: TFunction<"files">, language: string, chip: FileChip): string {
  const state = chip.state;
  switch (state.kind) {
    case "zipping":
      return t("composer.zipping", { percent: percent(state.done, state.total) });
    case "checking":
      return state.total > 0
        ? t("composer.checkingPercent", { percent: percent(state.done, state.total) })
        : t("composer.checking");
    case "failed":
      return chipErrorText(t, state.error);
    case "ready":
    case "sent":
      return t("composer.chipPeerToPeer", {
        size: formatSize(chip.size ?? 0, language),
      });
  }
}

/** Chips that are still being prepared hold the send back. */
export function isBusy(chip: FileChip): boolean {
  return chip.state.kind === "zipping" || chip.state.kind === "checking";
}
