import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ConnectionStats } from "../../bindings/commands";
import { useConnectionStats } from "./useConnectionStats";
import "./connection.css";

const SIGNAL = "M4 20v-3M9 20v-7M14 20v-11M19 20V4";

function StatsList({ stats }: { stats: ConnectionStats }) {
  const { t } = useTranslation("connection");
  const rows: [string, string][] = [
    [t("transport"), stats.transport ? t(`transportValue.${stats.transport}`) : t("none")],
    [
      t("latency"),
      stats.rttMs === null ? t("none") : t("latencyValue", { value: Math.round(stats.rttMs) }),
    ],
    [t("loss"), t("lossValue", { value: stats.lossPercent })],
    [t("bitrate"), t("bitrateValue", { value: stats.sendBitrateKbps })],
    [
      t("serverLimit"),
      stats.serverMaxBandwidthKbps === null
        ? t("none")
        : t("bitrateValue", { value: stats.serverMaxBandwidthKbps }),
    ],
    [t("packets"), t("packetsValue", { sent: stats.packetsSent, received: stats.packetsReceived })],
  ];
  return (
    <dl className="connection-info__list">
      {rows.map(([label, value]) => (
        <div key={label} className="connection-info__row">
          <dt>{label}</dt>
          <dd>{value}</dd>
        </div>
      ))}
    </dl>
  );
}

function Panel({ open }: { open: boolean }) {
  const { t } = useTranslation("connection");
  const state = useConnectionStats(open);
  switch (state.status) {
    case "loading":
      return <p className="connection-info__note">{t("loading")}</p>;
    case "unavailable":
      return <p className="connection-info__note">{t("notConnected")}</p>;
    case "error":
      return <p className="connection-info__note">{t("error")}</p>;
    case "ready":
      return <StatsList stats={state.stats} />;
  }
}

/**
 * Button with a non-modal popover showing the live link figures (AC-1.23): transport,
 * latency, loss and the current send bitrate. Refreshes about once per second while open.
 */
export function ConnectionInfo({ className }: { className?: string }) {
  const { t } = useTranslation("connection");
  const [open, setOpen] = useState(false);
  const panelId = useId();
  const titleId = useId();
  const wrapper = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    panel.current?.focus();
    const onPointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node && !wrapper.current?.contains(event.target)) setOpen(false);
    };
    // Escape closes from anywhere while open, returning focus to the button.
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      setOpen(false);
      button.current?.focus();
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  return (
    <div ref={wrapper} className="connection-info">
      <button
        ref={button}
        type="button"
        className={className}
        aria-label={t("open")}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-controls={open ? panelId : undefined}
        onClick={() => {
          setOpen((o) => !o);
        }}
      >
        <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" focusable="false">
          <path
            d={SIGNAL}
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
          />
        </svg>
      </button>
      {open ? (
        <div
          ref={panel}
          id={panelId}
          role="dialog"
          aria-labelledby={titleId}
          tabIndex={-1}
          className="connection-info__panel"
        >
          <h2 id={titleId} className="connection-info__title">
            {t("title")}
          </h2>
          <Panel open={open} />
        </div>
      ) : null}
    </div>
  );
}
