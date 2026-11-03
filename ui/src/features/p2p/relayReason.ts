/**
 * Plain-language texts for why the relay or a P2P feature can't be used (FR-4.4, FR-4.11,
 * FR-4.14). The same texts appear on the disabled "Watch" button, in the screen share viewer and
 * in the server info panel.
 */
import type { TFunction } from "i18next";
import type { RelayUnavailable, RelayUse, WatchBlock } from "../../api";

/** `t` of the "p2p" namespace. */
type P2pT = TFunction<"p2p">;

/** The p2p text key explaining that the relay doesn't allow a use (FR-7.7). */
const USE_REASON_KEY: Record<
  RelayUse,
  "imagesDisallowed" | "filesDisallowed" | "screenDisallowed"
> = {
  images: "imagesDisallowed",
  files: "filesDisallowed",
  screenShare: "screenDisallowed",
};

export function relayReasonText(t: P2pT, reason: RelayUnavailable): string {
  switch (reason.kind) {
    case "refused":
      return t("reason.refused", { reason: t(`refusal.${reason.refusal}`) });
    // "relay full" is a refusal of the relay (AC-4.17), whichever side reports it.
    case "relayFull":
      return t("reason.refused", { reason: t("refusal.relayFull") });
    case "useNotAllowed":
      return t(`relay.${USE_REASON_KEY[reason.use]}`);
    default:
      return t(`reason.${reason.kind}`);
  }
}

/**
 * Why "Watch" is unavailable, for the blocks that come from the P2P mode or the relay; `null`
 * for the older blocks, which the caller explains in its own words.
 */
export function p2pBlockText(
  t: P2pT,
  block: WatchBlock,
  relayReason: RelayUnavailable | null,
): string | null {
  if (block === "p2pOff") return t("reason.p2pOff");
  if (block === "relayUnavailable")
    return relayReasonText(t, relayReason ?? { kind: "unreachable" });
  return null;
}
