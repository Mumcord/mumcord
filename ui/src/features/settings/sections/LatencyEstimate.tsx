import { useTranslation } from "react-i18next";
import type { LatencyEstimate as Estimate } from "../../../bindings/commands";
import { formatMs } from "./formatMs";
import "./latency.css";

/** Headline figure plus the other mode's figure for comparison. */
export function LatencySummary({ estimate }: { estimate: Estimate }) {
  const { t, i18n } = useTranslation("common");
  const fmt = (us: number) => formatMs(us, i18n.language);
  return (
    <p className="latency-summary">
      {t("latency.summary", { total: fmt(estimate.totalUs) })} ·{" "}
      {t(estimate.lowLatency ? "latency.otherNormal" : "latency.otherLow", {
        value: fmt(estimate.otherModeTotalUs),
      })}
    </p>
  );
}

/** One stage of the mouth-to-ear path; `us` is `null` while it is unknown (no ping yet). */
interface Part {
  key: string;
  label: string;
  us: number | null;
}

/**
 * Where the time goes: one stacked bar, each stage a segment as wide as its share, with the
 * total next to it. Segments are focusable and show their name and value as a tooltip; the bar's
 * label lists every stage for screen readers.
 */
export function LatencyBreakdown({ estimate }: { estimate: Estimate }) {
  const { t, i18n } = useTranslation("common");
  const ms = (us: number) => t("latency.value", { value: formatMs(us, i18n.language) });
  const parts: Part[] = [
    { key: "input", label: t("latency.input"), us: estimate.inputBufferUs },
    {
      key: "encode",
      label: t("latency.encode", { ms: estimate.packetMs }),
      us: estimate.encodeUs,
    },
    { key: "network", label: t("latency.network"), us: estimate.networkUs },
    { key: "jitter", label: t("latency.jitter"), us: estimate.jitterUs },
    { key: "processing", label: t("latency.processing"), us: estimate.processingUs },
    { key: "output", label: t("latency.output"), us: estimate.outputBufferUs },
  ];
  const describe = (part: Part) =>
    `${part.label} ${part.us === null ? t("latency.notConnected") : ms(part.us)}`;
  const label = `${t("latency.breakdown")}: ${parts.map(describe).join(", ")}`;
  return (
    <div className="latency-bar-row">
      <div role="group" aria-label={label} className="latency-bar">
        {parts.map((part) =>
          part.us === null ? null : (
            <span
              key={part.key}
              // Focusable so keyboard users get the same tooltip as on hover.
              // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex
              tabIndex={0}
              role="img"
              aria-label={describe(part)}
              data-tooltip={describe(part)}
              data-tooltip-placement="bottom"
              className={`latency-bar__segment latency-bar__segment--${part.key}`}
              style={{ flexGrow: part.us }}
            />
          ),
        )}
      </div>
      <span className="latency-bar__total" aria-hidden="true">
        ≈ {ms(estimate.totalUs)}
      </span>
    </div>
  );
}
