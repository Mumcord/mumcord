/** The one reason the paperclip is disabled, in the order of FR-7.9. */
import type { TFunction } from "i18next";
import type { FilesUnavailable } from "../../../bindings/commands";

export function unavailableText(t: TFunction<"p2p">, reason: FilesUnavailable): string {
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
