/** Plain-language texts for why a file can't be downloaded or why a download failed. */
import type { TFunction } from "i18next";
import type { FailReason, FilesUnavailable } from "../../../api";
import { relayReasonText } from "../../p2p/relayReason";

/** Why file sharing is unavailable on the server, in the words of FR-7.9 (mockup 13). */
export function filesUnavailableText(t: TFunction<"p2p">, reason: FilesUnavailable): string {
  switch (reason.kind) {
    case "noRelay":
      return t("relay.noRelay");
    case "relayNoFiles":
      return t("relay.filesDisallowed");
    case "noCertificate":
      return t("relay.noCertificate");
    case "p2pMode":
      return t("relay.modeNeedsRelay", {
        mode: reason.mode === "off" ? t("relay.modeOff") : t("relay.modeDirectOnly"),
      });
  }
}

/** The `{reason}` of "Download failed: {reason}" (FR-7.20, FR-7.34). */
export function failReasonText(
  t: TFunction<"files">,
  tP2p: TFunction<"p2p">,
  reason: FailReason,
  sender: string,
): string {
  switch (reason.kind) {
    case "noAnswer":
      return t("card.reason.noAnswer", { sender });
    case "busy":
      return t("card.busy", { sender });
    case "connect":
      return reason.reason === null
        ? t("card.reason.connectUnknown")
        : t("card.reason.connect", { reason: relayReasonText(tP2p, reason.reason) });
    case "stalled":
      return t("card.reason.stalled");
    case "connectionLost":
      return t("card.reason.connectionLost");
    case "noSpace":
      return t("card.noSpace", { folder: reason.folder });
    case "io":
      return reason.message;
  }
}
