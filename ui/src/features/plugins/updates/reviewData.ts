import type { PluginBand, PluginScored, PluginUpdate } from "../../../api";

/** The new permissions this update asks for, in manifest order. */
export function newPermissionsOf(update: PluginUpdate): string[] {
  return update.approval?.kind === "newPermissions" ? update.approval.permissions : [];
}

export function newOriginCount(update: PluginUpdate): number {
  return update.review.origins.filter((origin) => origin.new).length;
}

/**
 * The update cannot be installed with one click: it waits for a decision and gets the review
 * dialog first (FR-6.67, FR-9.82, FR-9.83).
 */
export function needsReview(update: PluginUpdate): boolean {
  return (
    update.approval !== null ||
    update.hold !== null ||
    update.review.needsHighRiskConfirm ||
    newOriginCount(update) > 0
  );
}

/**
 * The chip components take the engine's `PluginScored`; the update review only carries the value
 * and band, which is all the chip shows. The breakdown fields are never read on this path.
 */
export function scoredOf(value: number, band: PluginBand): PluginScored {
  return { kind: "scored", score: { value, band, e: 0, f: null, h: 0, factors: [], version: 0 } };
}
