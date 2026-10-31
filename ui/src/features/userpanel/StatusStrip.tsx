import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useLowLatencySetting } from "../../app/preferences";
import { useActiveAudioProfile } from "../channelaudio";
import { ConnectionInfo } from "../connection";
import { DndIndicator } from "../dnd/DndIndicator";
import type { StripMessage } from "./stripMessage";
import { WhisperButton } from "../whisper/WhisperPanel";

const ALERT = "M12 4l9 16H3zM12 10v4M12 17v.01";
const SPINNER = "M12 3a9 9 0 1 0 9 9";
const PLUG = "M9 3v5M15 3v5M6 8h12v3a6 6 0 0 1-12 0zM12 17v4";
const ARROW = "M7 17L17 7M9 7h8v8";
const BOLT = "M13 2L4 14h7l-1 8 9-12h-7z";
const CLOSE = "M6 6l12 12M18 6L6 18";

function Glyph({ d, className }: { d: string; className: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" aria-hidden="true" focusable="false">
      <path
        d={d}
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

/** Text that ends in an ellipsis when cut off, with the full text as tooltip then. */
function Clipped({ children, full }: { children: ReactNode; full: string }) {
  return (
    <span className="userpanel-strip__text" data-tooltip={full} data-tooltip-overflow>
      {children}
    </span>
  );
}

interface StatusStripProps {
  message: StripMessage;
  error: string | null;
  serverLabel: string;
  /** Own channel on the voice server, `null` until known. */
  voiceChannel: string | null;
  voiceServerLabel: string;
  /** Own channel on the shown server, for the read-only Join button. */
  joinChannel: string | null;
  /** Attempt and limit while reconnecting, e.g. 2 of 10. */
  reconnect: { attempt: number; max: number } | null;
  connected: boolean;
  onDismissError: () => void;
  onJoin: () => void;
  onShowVoiceServer: () => void;
}

/**
 * The fixed-height second row of the user panel (#246): one message on the left, chosen by
 * `stripMessage`, and compact chips for persistent states on the right. Long texts end in an
 * ellipsis instead of wrapping, so the panel never changes height.
 */
export function StatusStrip(props: StatusStripProps) {
  const { message, connected } = props;
  const { t: tc } = useTranslation("connection");
  const globalLowLatency = useLowLatencySetting();
  const profile = useActiveAudioProfile();
  const lowLatency = connected && (profile?.profile.lowLatency ?? globalLowLatency);
  return (
    <div className="userpanel-strip" data-message={message}>
      <Message {...props} />
      <span className="userpanel-strip__chips">
        {/* Its own live region, so turning Do Not Disturb on is announced like before. */}
        <span role="status" className="userpanel-strip__persistent">
          <DndIndicator className="userpanel-chip" />
          {lowLatency ? (
            <button
              type="button"
              className="userpanel-chip userpanel-chip--icon userpanel-chip--latency"
              aria-label={tc("lowLatencyMode")}
              data-tooltip={tc("lowLatencyMode")}
            >
              <Glyph d={BOLT} className="userpanel-chip__icon" />
            </button>
          ) : null}
        </span>
        {connected ? (
          <WhisperButton className="userpanel-chip" iconClassName="userpanel-chip--icon" />
        ) : null}
        {connected ? (
          <ConnectionInfo className="userpanel-chip userpanel-chip--icon" compact />
        ) : null}
      </span>
    </div>
  );
}

function Message({
  message,
  error,
  serverLabel,
  voiceChannel,
  voiceServerLabel,
  joinChannel,
  reconnect,
  onDismissError,
  onJoin,
  onShowVoiceServer,
}: StatusStripProps) {
  const { t } = useTranslation("userpanel");
  switch (message) {
    case "error": {
      const text = error ?? "";
      return (
        <span className="userpanel-strip__message userpanel-error" role="alert">
          <Glyph d={ALERT} className="userpanel-strip__icon" />
          <Clipped full={text}>{text}</Clipped>
          <button
            type="button"
            className="userpanel-strip__dismiss"
            aria-label={t("dismissError")}
            data-tooltip={t("dismissError")}
            onClick={onDismissError}
          >
            <Glyph d={CLOSE} className="userpanel-strip__icon" />
          </button>
        </span>
      );
    }
    case "reconnecting":
    case "connecting":
    case "disconnected": {
      const text =
        message === "reconnecting" && reconnect
          ? `${t("status.reconnecting")} ${String(reconnect.attempt)}/${String(reconnect.max)}`
          : t(`status.${message}`);
      return (
        <span className="userpanel-strip__message" role="status">
          <Glyph
            d={message === "disconnected" ? PLUG : SPINNER}
            className={
              message === "disconnected"
                ? "userpanel-strip__icon"
                : "userpanel-strip__icon userpanel-strip__spinner"
            }
          />
          <Clipped full={text}>{text}</Clipped>
        </span>
      );
    }
    case "readOnly":
      return (
        <span className="userpanel-strip__message userpanel-readonly" role="status">
          <Clipped full={t("readOnly.text")}>{t("status.readOnly")}</Clipped>
          {joinChannel !== null ? (
            <button
              type="button"
              className="userpanel-strip__action userpanel-readonly__join"
              data-tooltip={t("readOnly.join", { channel: joinChannel })}
              data-tooltip-overflow
              onClick={onJoin}
            >
              {t("readOnly.join", { channel: joinChannel })}
            </button>
          ) : null}
        </span>
      );
    case "voiceElsewhere": {
      const text =
        voiceChannel === null
          ? t("voice.server", { server: voiceServerLabel })
          : t("voice.line", { channel: voiceChannel, server: voiceServerLabel });
      const label = t("voice.show", { location: text });
      return (
        <span className="userpanel-strip__message" role="status">
          <button
            type="button"
            className="userpanel-voice"
            aria-label={label}
            data-tooltip={label}
            onClick={onShowVoiceServer}
          >
            <Glyph d={ARROW} className="userpanel-strip__icon" />
            <span className="userpanel-strip__text">{text}</span>
          </button>
        </span>
      );
    }
    case "voice": {
      const full =
        voiceChannel === null
          ? t("voice.server", { server: serverLabel })
          : t("voice.line", { channel: voiceChannel, server: serverLabel });
      return (
        <span className="userpanel-strip__message" role="status">
          <span className="userpanel-strip__dot" aria-hidden="true" />
          {/* The server already is the subtitle under the name, so only the channel shows; screen
              readers get the whole location. */}
          <span className="userpanel-strip__text" data-tooltip={full}>
            <span className="visually-hidden">{full}</span>
            <span aria-hidden="true">{voiceChannel ?? serverLabel}</span>
          </span>
        </span>
      );
    }
  }
}
