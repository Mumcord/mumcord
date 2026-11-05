import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { PluginEffect, PluginScore } from "../../../bindings/commands";
import {
  breakdownRows,
  floorPoints,
  type BreakdownRow,
  type FactorGroup,
  type Found,
} from "./breakdownRows";
import { useFactorName } from "./useFactorName";
import "./score.css";

/** Versions the "recent changes" part looks back over (FR-9.73, formula version 1). */
const HISTORY_VERSIONS = 5;

/**
 * Facts about the package that the "What Mumcord found" column shows when the caller knows them
 * (from the index entry or the verified package). They only describe; they never change a number.
 */
export interface ScoreFacts {
  /** Number of declared websites (`net.origins`). */
  websites?: number;
  /** Package size, already formatted (e.g. "12 MiB"). */
  packageSize?: string;
  wasmModules?: number;
  /** Days this device has known the author key (FR-9.74). */
  keyAgeDays?: number;
}

export interface ScoreBreakdownProps {
  score: PluginScore;
  /** Names the breakdown for screen readers ("How the security score of … is made up"). */
  pluginName: string;
  facts?: ScoreFacts;
  /**
   * Wraps the tables in "Why this score", open for Medium and High, closed for Low (FR-9.77,
   * details page). Without it the tables show directly (install prompt, "Show the full breakdown").
   */
  collapsible?: boolean;
  /** Opens the formula documentation; the link is left out when not given. */
  onHowCalculated?: (() => void) | undefined;
}

/** Visible arrow and number plus one spoken sentence (§5.6: "plus 20 points, adds risk"). */
export function EffectText({ effect }: { effect: PluginEffect }) {
  const { t } = useTranslation("pluginScore");
  if (effect.value === 0) {
    return (
      <span className="risk-eff" data-dir="flat">
        {t("effect.none")}
      </span>
    );
  }
  const up = effect.value > 0;
  const count = Math.abs(effect.value);
  const percent = effect.kind === "percent";
  const shown = percent
    ? t("effect.percentLabel", { sign: up ? "+" : "−", count })
    : up
      ? t("effect.pointsLabel", { count })
      : `−${String(count)}`;
  const spoken = percent
    ? t(up ? "effect.percentUp" : "effect.percentDown", { count })
    : t(up ? "effect.pointsUp" : "effect.pointsDown", { count });
  return (
    <span className="risk-eff" data-dir={up ? "up" : "down"}>
      <span aria-hidden="true">
        {up ? "▲" : "▼"} {shown}
      </span>
      <span className="visually-hidden">{spoken}</span>
    </span>
  );
}

function FoundText({ found, facts }: { found: Found; facts: ScoreFacts }): ReactNode {
  const { t } = useTranslation("pluginScore");
  switch (found.kind) {
    case "permission": {
      const extra: string[] = [];
      if (found.firstWebSurface) extra.push(t("breakdown.firstWebSurface"));
      if (found.id === "net.origins" && facts.websites !== undefined) {
        extra.push(t("breakdown.websitesCount", { count: facts.websites }));
      }
      return (
        <>
          <code>{found.id}</code>
          {extra.map((text) => ` · ${text}`).join("")}
        </>
      );
    }
    case "websites":
      return facts.websites === undefined
        ? null
        : t("breakdown.websitesDeclared", { count: facts.websites });
    case "package":
      if (facts.packageSize !== undefined && facts.wasmModules !== undefined) {
        return t("breakdown.packageSizeValue", {
          size: facts.packageSize,
          count: facts.wasmModules,
        });
      }
      return facts.packageSize ?? null;
    case "keyAge":
      if (facts.keyAgeDays !== undefined) {
        return t("breakdown.authorKeyDays", { count: facts.keyAgeDays });
      }
      return found.detailKey === "key_age.new" ? t("breakdown.authorKeyNew") : null;
    case "text":
      return t(found.key);
    case "noneRecent":
      return t("breakdown.newPermissionsNone", { count: HISTORY_VERSIONS });
    case "none":
      return null;
  }
}

const GROUPS: { group: FactorGroup; caption: string; effectHeader: string | null }[] = [
  {
    group: "capabilities",
    caption: "breakdown.groupCapabilities",
    effectHeader: "breakdown.points",
  },
  { group: "trust", caption: "breakdown.groupTrust", effectHeader: "breakdown.adjustment" },
  // Mockup 02: the history table has no column headers, only its row header.
  { group: "history", caption: "breakdown.groupHistory", effectHeader: null },
];

function GroupTable({
  caption,
  effectHeader,
  rows,
  facts,
}: {
  caption: string;
  effectHeader: string | null;
  rows: readonly BreakdownRow[];
  facts: ScoreFacts;
}) {
  const { t } = useTranslation("pluginScore");
  const factorName = useFactorName();
  return (
    <table className="risk-table">
      <caption>{t(caption)}</caption>
      {effectHeader !== null && (
        <thead>
          <tr>
            <th scope="col">{t("breakdown.factor")}</th>
            <th scope="col">{t("breakdown.found")}</th>
            <th scope="col">{t(effectHeader)}</th>
          </tr>
        </thead>
      )}
      <tbody>
        {rows.map((row, i) => (
          <tr key={i}>
            <th scope="row">{factorName(row.name)}</th>
            <td>
              <FoundText found={row.found} facts={facts} />
            </td>
            <td>
              <EffectText effect={row.effect} />
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** Placeholder the score is swapped for, so the result can be bold inside a translated sentence. */
const SCORE_SLOT = "\u0000";

function Formula({
  score,
  onHowCalculated,
}: {
  score: PluginScore;
  onHowCalculated?: (() => void) | undefined;
}) {
  const { t, i18n } = useTranslation("pluginScore");
  const factor = score.f ?? 1;
  const percent = Math.round((factor - 1) * 100);
  const floor = floorPoints(score.factors);
  const [before = "", after = ""] = t("breakdown.formula", {
    points: score.e,
    factor: new Intl.NumberFormat(i18n.language, {
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    }).format(factor),
    percent: t("effect.percentLabel", {
      sign: percent > 0 ? "+" : percent < 0 ? "−" : "",
      count: Math.abs(percent),
    }),
    history: score.h,
    // The formula line shows the sum before the floor; the floor sentence explains the rest.
    score: SCORE_SLOT,
  }).split(SCORE_SLOT);
  return (
    <p className="risk-formula">
      {before}
      <strong>{score.value - floor}</strong>
      {after}.{floor > 0 && <> {t("breakdown.floor", { score: score.value })}</>}{" "}
      {t("breakdown.formulaVersion", { version: score.version })}
      {onHowCalculated !== undefined && (
        <>
          {" · "}
          <button type="button" className="risk-link" onClick={onHowCalculated}>
            {t("breakdown.howCalculated")}
          </button>
        </>
      )}
    </p>
  );
}

/**
 * "Why this score" (FR-9.77, mockup 02): one real table per part of the formula, then the
 * formula line with its version. Shows the engine's values only.
 */
export function ScoreBreakdown({
  score,
  pluginName,
  facts = {},
  collapsible = false,
  onHowCalculated,
}: ScoreBreakdownProps) {
  const { t } = useTranslation("pluginScore");
  const rows = breakdownRows(score.factors);
  // The engine lists no history row when it has history but found no new permissions.
  if (!rows.some((r) => r.group === "history")) {
    rows.push({
      group: "history",
      name: { kind: "text", key: "breakdown.newPermissions" },
      found: { kind: "noneRecent" },
      effect: { kind: "points", value: 0 },
    });
  }
  const body = (
    <div
      className="risk-breakdown"
      role="group"
      aria-label={t("breakdown.caption", { plugin: pluginName })}
    >
      {GROUPS.map(({ group, caption, effectHeader }) => (
        <GroupTable
          key={group}
          caption={caption}
          effectHeader={effectHeader}
          rows={rows.filter((r) => r.group === group)}
          facts={facts}
        />
      ))}
      <Formula score={score} onHowCalculated={onHowCalculated} />
    </div>
  );
  if (!collapsible) return body;
  return (
    <details className="risk-details" open={score.band !== "low"}>
      <summary>{t("breakdown.title")}</summary>
      {body}
    </details>
  );
}
