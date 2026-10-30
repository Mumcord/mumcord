import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import type { AudioHealth, LatencyEstimate } from "../../bindings/commands";
import {
  FRAME_BUDGET_MS,
  concealedShare,
  loadDetailsOpen,
  perMinute,
  saveDetailsOpen,
  windowMinutes,
} from "./audioHealth";
import { LatencyBreakdown } from "../settings/sections/LatencyEstimate";

const SPARK_WIDTH = 32;
const SPARK_HEIGHT = 14;

/** Tiny trend line of the recent window; the numbers next to it carry the meaning. */
function Sparkline({ points }: { points: number[] }) {
  const peak = Math.max(1, ...points);
  const step = points.length > 1 ? SPARK_WIDTH / (points.length - 1) : 0;
  const coords = points
    .map((value, i) => {
      const y = SPARK_HEIGHT - 1 - (value / peak) * (SPARK_HEIGHT - 2);
      return `${(i * step).toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
  return (
    <svg
      className="audio-details__spark"
      viewBox={`0 0 ${String(SPARK_WIDTH)} ${String(SPARK_HEIGHT)}`}
      width={SPARK_WIDTH}
      height={SPARK_HEIGHT}
      aria-hidden="true"
      focusable="false"
    >
      <polyline points={coords} fill="none" stroke="currentColor" strokeWidth="1.5" />
    </svg>
  );
}

interface Row {
  key: string;
  label: string;
  value: string;
  /** What the counter means and what is normal, plus the recent figure when there is one. */
  help: string;
  history?: number[];
}

function DetailRow({ row }: { row: Row }) {
  const { t } = useTranslation("connection");
  const helpId = useId();
  return (
    <div className="connection-info__row">
      <dt className="audio-details__label">
        <span>{row.label}</span>
        <button
          type="button"
          className="audio-details__info"
          aria-label={t("about", { label: row.label })}
          aria-describedby={helpId}
          data-tooltip={row.help}
        >
          <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true" focusable="false">
            <path
              d="M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18zM12 11v6M12 7.5v.01"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
            />
          </svg>
        </button>
        <span id={helpId} className="visually-hidden">
          {row.help}
        </span>
      </dt>
      <dd className="audio-details__value">
        <span>{row.value}</span>
        {row.history ? (
          <Sparkline points={row.history} />
        ) : (
          <span className="audio-details__spark" />
        )}
      </dd>
    </div>
  );
}

function useRows(audio: AudioHealth): Row[] {
  const { t, i18n } = useTranslation("connection");
  const oneDecimal = new Intl.NumberFormat(i18n.language, {
    minimumFractionDigits: 1,
    maximumFractionDigits: 1,
  });
  const ms = (us: number) => oneDecimal.format(us / 1000);
  const minutes = windowMinutes(audio.windowSecs);
  const rate = (count: number) =>
    t("recent.perMinute", {
      minutes,
      value: oneDecimal.format(perMinute(count, audio.windowSecs)),
    });
  const help = (key: string, recent?: string) =>
    recent === undefined ? t(`help.${key}`) : `${t(`help.${key}`)} ${recent}`;
  return [
    {
      key: "dropouts",
      label: t("audioDropouts"),
      value: String(audio.dropouts),
      help: help("dropouts", rate(audio.recentDropouts)),
      history: audio.dropoutsHistory,
    },
    {
      key: "late",
      label: t("audioLate"),
      value: String(audio.lateFrames),
      help: help("late", rate(audio.recentLateFrames)),
      history: audio.lateFramesHistory,
    },
    {
      key: "concealed",
      label: t("audioConcealed"),
      value: String(audio.concealedFrames),
      help: help(
        "concealed",
        t("recent.concealedShare", { minutes, value: oneDecimal.format(concealedShare(audio)) }),
      ),
      history: audio.concealedHistory,
    },
    {
      key: "jitter",
      label: t("jitterBuffer"),
      value: t("latencyValue", { value: audio.jitterBufferMs }),
      help: help("jitter"),
    },
    {
      key: "processing",
      label: t("audioProcessing"),
      value: t("audioProcessingValue", {
        avg: ms(audio.processingAvgUs),
        max: ms(audio.processingMaxUs),
      }),
      help: help(
        "processing",
        t("recent.processingMax", {
          minutes,
          value: ms(audio.recentProcessingMaxUs),
          budget: FRAME_BUDGET_MS,
        }),
      ),
      history: audio.processingMaxHistoryUs,
    },
    {
      key: "realtime",
      label: t("realtimeAudio"),
      value: t(audio.realtime ? "realtimeValue.on" : "realtimeValue.off"),
      help: help("realtime"),
    },
  ];
}

/** The raw counters behind the rating, behind a disclosure that remembers its state. */
export function AudioHealthDetails({
  audio,
  latency,
}: {
  audio: AudioHealth;
  latency: LatencyEstimate | null;
}) {
  const { t } = useTranslation("connection");
  const [open, setOpen] = useState(loadDetailsOpen);
  const contentId = useId();
  const rows = useRows(audio);
  return (
    <div className="audio-details">
      <button
        type="button"
        className="audio-details__toggle"
        aria-expanded={open}
        aria-controls={contentId}
        onClick={() => {
          setOpen(!open);
          saveDetailsOpen(!open);
        }}
      >
        <svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true" focusable="false">
          <path d="M8 5l9 7-9 7z" fill="currentColor" />
        </svg>
        {t("details")}
      </button>
      <div id={contentId} hidden={!open} className="audio-details__content">
        <dl className="connection-info__list">
          {rows.map((row) => (
            <DetailRow key={row.key} row={row} />
          ))}
        </dl>
        {latency ? <LatencyBreakdown estimate={latency} /> : null}
        <p className="audio-details__note">
          {t("sinceConnected", { minutes: windowMinutes(audio.windowSecs) })}
        </p>
      </div>
    </div>
  );
}
