import type { PluginBand, PluginScored, PluginUpdate, PluginUpdateReview } from "../../../api";

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

/** Second-level labels under which a registry hands out names, as in `example.co.uk`. */
const REGISTRY_LABELS = new Set(["co", "com", "org", "net", "gov", "ac", "edu"]);

/**
 * The registrable domain of an origin (`https://api.example.co.uk:8443` → `example.co.uk`). A
 * close match of the engine's public-suffix lookup, used only to decide how a purpose is shown.
 */
function registrableDomain(origin: string): string {
  let host = origin;
  try {
    host = new URL(origin).hostname;
  } catch {
    // An unparsable origin is compared as written.
  }
  const labels = host.split(".");
  const registry = labels.length > 2 && REGISTRY_LABELS.has(labels[labels.length - 2] ?? "");
  return labels.slice(registry ? -3 : -2).join(".");
}

/**
 * New websites outside every domain the plugin already used (FR-9.83). Their purposes are the
 * plugin's own words about a site that is not its own, so the dialog quotes them.
 */
export function offDomainOrigins(review: PluginUpdateReview): ReadonlySet<string> {
  if (!review.otherDomain) return new Set();
  const known = new Set(
    review.origins.filter((o) => !o.new).map((o) => registrableDomain(o.origin)),
  );
  return new Set(
    review.origins
      .filter((o) => o.new && !known.has(registrableDomain(o.origin)))
      .map((o) => o.origin),
  );
}
