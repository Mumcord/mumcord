import { useContext, useEffect, useId, useRef, useState } from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import type { ChannelAudioProfile, ConnectionStats } from "../../bindings/commands";
import { useActiveAudioProfile } from "../channelaudio";
import { shownServerId } from "../channels/serverStore";
import { useServers } from "../servers";
import { windowMinutes } from "./audioHealth";
import { AudioHealthDetails } from "./AudioHealthDetails";
import { ViewContext } from "../../app/view";
import { AudioHealthRating } from "./AudioHealthRating";
import { formatMs } from "../settings/sections/formatMs";
import { ConnectionQuickPanel } from "./ConnectionQuickPanel";
import { useConnectionStats } from "./useConnectionStats";
import "./connection.css";

const SIGNAL = "M4 20v-3M9 20v-7M14 20v-11M19 20V4";

/** The values a channel audio profile sets, e.g. "Music quality, 256 kbit/s (channel profile)". */
function profileText(profile: ChannelAudioProfile, t: TFunction<"connection">) {
  const items = [
    profile.musicQuality === null
      ? null
      : t(profile.musicQuality ? "profile.musicOn" : "profile.musicOff"),
    profile.lowLatency === null
      ? null
      : t(profile.lowLatency ? "profile.lowLatencyOn" : "profile.lowLatencyOff"),
    profile.bitrateKbps === null ? null : t("bitrateValue", { value: profile.bitrateKbps }),
  ].filter((item) => item !== null);
  return t("profileSource", {
    label: items.length > 0 ? items.join(", ") : t("profile.defaults"),
  });
}

function StatsList({ stats }: { stats: ConnectionStats }) {
  const { t, i18n } = useTranslation("connection");
  const active = useActiveAudioProfile();
  const rows: [string, string][] = [
    [t("transport"), stats.transport ? t(`transportValue.${stats.transport}`) : t("none")],
    [
      t("latency"),
      stats.rttMs === null ? t("none") : t("latencyValue", { value: Math.round(stats.rttMs) }),
    ],
    ...(stats.latency
      ? [
          [
            t("endToEnd"),
            (stats.latency.lowLatency ? `${t("lowLatencyMode")} · ` : "") +
              t("endToEndValue", { value: formatMs(stats.latency.totalUs, i18n.language) }),
          ] as [string, string],
        ]
      : []),
    [t("loss"), t("lossValue", { value: stats.lossPercent })],
    [t("bitrate"), t("bitrateValue", { value: stats.sendBitrateKbps })],
    ...(active ? [[t("profile.row"), profileText(active.profile, t)] as [string, string]] : []),
    [
      t("serverLimit"),
      stats.serverMaxBandwidthKbps === null
        ? t("none")
        : t("bitrateValue", { value: stats.serverMaxBandwidthKbps }),
    ],
    [t("packets"), t("packetsValue", { sent: stats.packetsSent, received: stats.packetsReceived })],
  ];
  return (
    <>
      <dl className="connection-info__list">
        {rows.map(([label, value]) => (
          <div key={label} className="connection-info__row">
            <dt>{label}</dt>
            <dd>{value}</dd>
          </div>
        ))}
      </dl>
      {stats.audio ? (
        <>
          <AudioHealthRating
            rating={stats.audio.rating}
            issue={stats.audio.issue}
            minutes={windowMinutes(stats.audio.windowSecs)}
          />
          <AudioHealthDetails audio={stats.audio} latency={stats.latency} />
        </>
      ) : null}
    </>
  );
}

/** Short fingerprint as Mumble shows it: the first four groups. */
function shortFingerprint(certHash: string): string {
  return (certHash.match(/.{1,2}/g) ?? []).slice(0, 4).join(":");
}

/**
 * The identity the shown server connects with (issue #148). Read from local data, so it shows
 * even while the connection figures are unavailable.
 */
function IdentityRow() {
  const { t } = useTranslation("connection");
  const state = useServers();
  const server = state.servers.find((s) => s.id === shownServerId());
  const identity =
    state.identities.find((i) => i.id === (server?.identityId ?? -1)) ??
    state.identities.find((i) => i.isDefault);
  if (!identity) return null;
  return (
    <dl className="connection-info__list">
      <div className="connection-info__row">
        <dt>{t("identity")}</dt>
        <dd>
          {t("identityValue", {
            label: identity.label,
            name: identity.name,
            fingerprint: shortFingerprint(identity.certHash),
          })}
        </dd>
      </div>
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
 * `compact` (the user panel's status strip, #246) leaves out the separate arrow: the voice
 * quality panel then opens from the same button with right-click, the menu key or Shift+F10.
 */
export function ConnectionInfo({
  className,
  compact = false,
}: {
  className?: string;
  compact?: boolean;
}) {
  const { t } = useTranslation("connection");
  // "stats" is the statistics popover, "quick" the voice quality panel (right-click or menu key).
  const [mode, setMode] = useState<"stats" | "quick" | null>(null);
  const open = mode !== null;
  // The quick panel is wider than the sidebar, which clips its children, so it is placed in
  // viewport coordinates above the sidebar's left edge instead.
  const [quickPlace, setQuickPlace] = useState<{ left: number; bottom: number } | null>(null);
  const openQuick = () => {
    const anchor = wrapper.current?.offsetParent?.getBoundingClientRect();
    const trigger = button.current?.getBoundingClientRect();
    if (anchor && trigger) {
      setQuickPlace({
        left: anchor.left + 8,
        bottom: window.innerHeight - trigger.top + 8,
      });
    }
    setMode("quick");
  };
  const view = useContext(ViewContext);
  const panelId = useId();
  const titleId = useId();
  const wrapper = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    panel.current?.focus();
    const onPointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node && !wrapper.current?.contains(event.target)) setMode(null);
    };
    // Escape closes from anywhere while open, returning focus to the button.
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      // Escape first dismisses an open "?" tooltip; the panel closes with the next one.
      if (document.querySelector('[role="tooltip"]')) return;
      event.preventDefault();
      setMode(null);
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
        data-tooltip={compact ? t("open") : undefined}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-controls={open ? panelId : undefined}
        onClick={() => {
          setMode((m) => (m === "stats" ? null : "stats"));
        }}
        onContextMenu={(event) => {
          event.preventDefault();
          openQuick();
        }}
        onKeyDown={(event) => {
          // The menu key and Shift+F10 normally fire a context menu event; handle them directly so
          // it works the same everywhere.
          if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) {
            event.preventDefault();
            openQuick();
          }
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
      {compact ? null : (
        <button
          type="button"
          className={`${className ?? ""} connection-info__more`}
          aria-label={t("quick.open")}
          aria-haspopup="dialog"
          aria-expanded={mode === "quick"}
          onClick={() => {
            if (mode === "quick") setMode(null);
            else openQuick();
          }}
        >
          <svg viewBox="0 0 24 24" width="10" height="10" aria-hidden="true" focusable="false">
            <path d="M6 9l6 6 6-6" fill="none" stroke="currentColor" strokeWidth="3" />
          </svg>
        </button>
      )}
      {open ? (
        <div
          ref={panel}
          id={panelId}
          role="dialog"
          aria-labelledby={mode === "quick" ? undefined : titleId}
          aria-label={mode === "quick" ? t("quick.title") : undefined}
          tabIndex={-1}
          style={
            mode === "quick" && quickPlace
              ? { left: quickPlace.left, bottom: quickPlace.bottom, right: "auto" }
              : undefined
          }
          className={`connection-info__panel${mode === "quick" ? " connection-info__panel--quick" : ""}`}
        >
          {mode === "quick" ? null : (
            <h2 id={titleId} className="connection-info__title">
              {t("title")}
            </h2>
          )}
          {mode === "quick" ? (
            <ConnectionQuickPanel
              onOpenVoiceSettings={() => {
                setMode(null);
                view?.openAudioSettings();
              }}
            />
          ) : (
            <>
              <IdentityRow />
              <Panel open={open} />
            </>
          )}
        </div>
      ) : null}
    </div>
  );
}
