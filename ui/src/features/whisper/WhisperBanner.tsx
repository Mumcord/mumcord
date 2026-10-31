import { Trans, useTranslation } from "react-i18next";
import { useServerStateOf, useVoiceServerId } from "../channels/serverStore";
import { WhisperKindIcon } from "./WhisperKindIcon";
import {
  dismissWhisperDenial,
  stopWhisper,
  useOwnWhisper,
  useWhisperSetup,
  whisperKind,
} from "./whisperStore";
import "./whisper.css";

function reportStop(error: unknown): void {
  console.error("Stopping the whisper failed", error);
}

/**
 * Above the chat while the user whispers or shouts (issue #67, mockup `67-whisper-live`): says to
 * whom, how to get back to the own channel, and stops it. A refused start shows why instead.
 */
export function WhisperBanner() {
  const { t } = useTranslation("whisper");
  const own = useOwnWhisper();
  const setup = useWhisperSetup();
  const voice = useServerStateOf(useVoiceServerId());
  if (own.state === "idle") return null;
  const target = setup?.targets.find((candidate) => candidate.id === own.targetId) ?? null;
  const targetName = target?.name ?? "";

  if (own.state === "denied") {
    return (
      <div className="whisper-banner" data-state="denied" role="alert">
        <div className="whisper-banner__text">
          <p className="whisper-banner__title">{t("denied.title", { target: targetName })}</p>
          <p className="whisper-banner__hint">
            {t(`denied.${own.reason}`, { channel: own.channel ?? "" })}
          </p>
        </div>
        <button
          type="button"
          className="button whisper-banner__stop"
          onClick={dismissWhisperDenial}
        >
          {t("denied.dismiss")}
        </button>
      </div>
    );
  }

  const kind = target === null ? "whisper" : whisperKind(target.receivers);
  const self = voice.selfSession === null ? undefined : voice.users.get(voice.selfSession);
  const channel = self === undefined ? null : (voice.channels.get(self.channelId)?.name ?? null);
  return (
    <div className="whisper-banner" data-state="active" role="status">
      <WhisperKindIcon kind={kind} />
      <div className="whisper-banner__text">
        <p className="whisper-banner__title">{t(`banner.${kind}`, { target: targetName })}</p>
        <p className="whisper-banner__hint">
          {channel === null ? (
            t("banner.onlyThere")
          ) : own.held && target?.hotkey ? (
            <Trans
              ns="whisper"
              i18nKey="banner.releaseKey"
              values={{ key: target.hotkey, channel }}
              components={{ key: <kbd className="whisper-banner__key" /> }}
            />
          ) : (
            t("banner.stopToReturn", { channel })
          )}
        </p>
      </div>
      <button
        type="button"
        className="button whisper-banner__stop"
        onClick={() => {
          stopWhisper().catch(reportStop);
        }}
      >
        {t("banner.stop")}
      </button>
    </div>
  );
}
