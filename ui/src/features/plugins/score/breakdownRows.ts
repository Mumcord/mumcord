import type { PluginEffect, PluginScoreFactor } from "../../../bindings/commands";

/** The three parts of the formula, one table each (mockup 02). */
export type FactorGroup = "capabilities" | "trust" | "history";

/** What the "What Mumcord found" column says about a row. */
export type Found =
  | { kind: "permission"; id: string; firstWebSurface: boolean }
  | { kind: "websites" }
  | { kind: "package" }
  | { kind: "text"; key: string }
  | { kind: "keyAge"; detailKey: string }
  /** History is known and no permission was added within the window. */
  | { kind: "noneRecent" }
  | { kind: "none" };

/** What the "Factor" column names. */
export type FactorName =
  | { kind: "permission"; id: string }
  | { kind: "text"; key: string }
  /** A detail key this version does not know yet: shown as is, never hidden. */
  | { kind: "raw"; text: string };

export interface BreakdownRow {
  group: FactorGroup;
  name: FactorName;
  found: Found;
  effect: PluginEffect;
}

/** Display order of the capability rows (mockup 02): web surfaces first, then by weight class. */
const CAPABILITY_ORDER: Record<string, number> = {
  webSurface: 0,
  highPermission: 1,
  relay: 2,
  mediumPermission: 3,
  lowPermissions: 4,
  websites: 5,
  packageSize: 6,
  wasmModules: 6,
};

const PERMISSION_KINDS = new Set([
  "highPermission",
  "webSurface",
  "relay",
  "mediumPermission",
  "lowPermissions",
]);

/** Breakdown keys of the trust rows, by the engine's detail key. */
const TRUST_TEXTS: Record<string, { name: string; found: string }> = {
  "source_link.given": { name: "sourceLink", found: "sourceLinkGiven" },
  "source_link.missing": { name: "sourceLink", found: "sourceLinkMissing" },
  "licence.osi": { name: "licence", found: "licenceOpen" },
  "licence.proprietary": { name: "licence", found: "licenceProprietary" },
  "licence.not_declared": { name: "licence", found: "licenceNone" },
  "reproducible.claimed": { name: "reproducible", found: "reproducibleClaimed" },
  "reproducible.none": { name: "reproducible", found: "reproducibleNone" },
  "expected_key.matched": { name: "expectedKey", found: "expectedKeyMatched" },
};

/**
 * Turns the engine's factor list into table rows. Nothing is weighed here: every number comes
 * from the engine. The only merge is presentational: package size and module count share one
 * row ("Package size and code modules"), so their points are shown together as in mockup 02.
 * The network floor is not a row; the formula line explains it.
 */
export function breakdownRows(factors: readonly PluginScoreFactor[]): BreakdownRow[] {
  const rows: BreakdownRow[] = [];
  // The engine lists permissions by id; the table groups them as the mockup does (stable sort).
  const ordered = [...factors].sort(
    (a, b) => (CAPABILITY_ORDER[a.kind] ?? 9) - (CAPABILITY_ORDER[b.kind] ?? 9),
  );
  let packageRow: BreakdownRow | undefined;
  let webSurfaceSeen = false;
  for (const factor of ordered) {
    const { kind, effect, detailKey } = factor;
    if (PERMISSION_KINDS.has(kind)) {
      const first = kind === "webSurface" && !webSurfaceSeen;
      if (kind === "webSurface") webSurfaceSeen = true;
      rows.push({
        group: "capabilities",
        name: { kind: "permission", id: detailKey },
        found: { kind: "permission", id: detailKey, firstWebSurface: first },
        effect,
      });
    } else if (kind === "websites") {
      rows.push({
        group: "capabilities",
        name: { kind: "text", key: "breakdown.moreThanOneWebsite" },
        found: { kind: "websites" },
        effect,
      });
    } else if (kind === "packageSize" || kind === "wasmModules") {
      if (packageRow === undefined) {
        packageRow = {
          group: "capabilities",
          name: { kind: "text", key: "breakdown.packageSize" },
          found: { kind: "package" },
          effect,
        };
        rows.push(packageRow);
      } else {
        packageRow.effect = { kind: "points", value: packageRow.effect.value + effect.value };
      }
    } else if (kind === "keyAge") {
      rows.push({
        group: "trust",
        name: { kind: "text", key: "breakdown.authorKey" },
        found: { kind: "keyAge", detailKey },
        effect,
      });
    } else if (kind === "developerTrust") {
      rows.push({
        group: "trust",
        name: { kind: "text", key: "developerPlugin" },
        found: { kind: "text", key: "developerNote" },
        effect,
      });
    } else if (kind === "recentChange") {
      rows.push({
        group: "history",
        name: { kind: "permission", id: detailKey },
        found: { kind: "permission", id: detailKey, firstWebSurface: false },
        effect,
      });
    } else if (kind === "noHistory") {
      rows.push({
        group: "history",
        name: { kind: "text", key: "breakdown.newPermissions" },
        found: { kind: "text", key: "breakdown.noHistory" },
        effect,
      });
    } else if (kind !== "networkFloor") {
      const texts = TRUST_TEXTS[detailKey];
      rows.push(
        texts === undefined
          ? {
              group: "trust",
              name: { kind: "raw", text: detailKey },
              found: { kind: "none" },
              effect,
            }
          : {
              group: "trust",
              name: { kind: "text", key: `breakdown.${texts.name}` },
              found: { kind: "text", key: `breakdown.${texts.found}` },
              effect,
            },
      );
    }
  }
  return rows;
}

/** Points the engine added to lift a network plugin to the floor (FR-9.73), or 0. */
export function floorPoints(factors: readonly PluginScoreFactor[]): number {
  return factors.find((f) => f.kind === "networkFloor")?.effect.value ?? 0;
}

/**
 * The biggest reasons of the install prompt (mockup 03): the three largest point factors that
 * add risk, then the largest trust factor that adds risk. Only sorts the engine's values.
 */
export function biggestReasons(rows: readonly BreakdownRow[]): BreakdownRow[] {
  const byValue = (a: BreakdownRow, b: BreakdownRow) => b.effect.value - a.effect.value;
  const points = rows
    .filter((r) => r.effect.kind === "points" && r.effect.value > 0)
    .sort(byValue)
    .slice(0, 3);
  const trust = rows
    .filter((r) => r.effect.kind === "percent" && r.effect.value > 0)
    .sort(byValue)
    .slice(0, 1);
  return [...points, ...trust];
}
