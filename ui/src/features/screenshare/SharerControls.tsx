/**
 * The sharer's own controls while sharing (FR-2.43, FR-2.45): who is watching, "share full" at
 * 8 viewers, pause, sound on/off and stop. When the started share has no sound source a hint
 * says so instead of the sound switch (FR-2.42).
 */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type ScreenShare } from "../../api";
import { useServerStateOf } from "../channels";
import { ShareScreenButton } from "./ShareScreenButton";
import type { OwnShareState } from "./useOwnShare";

/** Viewers per share (FR-2.43); the engine turns away the 9th. */
export const MAX_VIEWERS = 8;

export function Viewers({ serverId, own }: { serverId: number; own: ScreenShare }) {
  const { t } = useTranslation("screenshare");
  const { users } = useServerStateOf(serverId);
  const count = own.viewers.length;
  if (count === 0) return <p className="sharer-controls__viewers">{t("viewersNone")}</p>;
  const names = own.viewers.map((s) => users.get(s)?.name ?? t("someone")).join(", ");
  const full = count >= MAX_VIEWERS;
  return (
    <p className="sharer-controls__viewers" data-full={full || undefined}>
      {full ? t("full", { max: MAX_VIEWERS, names }) : t("viewers", { count, names })}
    </p>
  );
}

export function SharerControls({ share }: { share: OwnShareState }) {
  const { t } = useTranslation("screenshare");
  const [changeFailed, setChangeFailed] = useState(false);
  const { serverId, own, platform } = share;
  if (serverId === null || own === null) return null;

  const change = (run: Promise<null>) => {
    setChangeFailed(false);
    run.catch((error: unknown) => {
      console.error("Changing the screen share failed", error);
      setChangeFailed(true);
    });
  };
  return (
    <section className="sharer-controls" aria-label={t("controls")}>
      <p className="sharer-controls__status">
        <span className="live-badge" aria-hidden="true">
          {t("live")}
        </span>{" "}
        {t("youreLive")}
        {own.paused && <span className="sharer-controls__paused"> · {t("paused")}</span>}
      </p>
      <Viewers serverId={serverId} own={own} />
      {!own.soundAvailable && (
        <p className="sharer-controls__hint">
          {t(platform?.sound === false ? "noSound" : "noSoundForShare")}
        </p>
      )}
      <div className="sharer-controls__buttons">
        <button
          type="button"
          className="button"
          onClick={() => {
            change(getApi().screenShareSetPaused(serverId, !own.paused));
          }}
        >
          {own.paused ? t("resume") : t("pause")}
        </button>
        {own.soundAvailable && (
          <button
            type="button"
            className="button"
            aria-pressed={own.sound}
            onClick={() => {
              change(getApi().screenShareSetSound(serverId, !own.sound));
            }}
          >
            {t("sound")}
          </button>
        )}
        <ShareScreenButton share={share} />
      </div>
      {changeFailed && (
        <p className="share-button__error" role="alert">
          {t("changeFailed")}
        </p>
      )}
    </section>
  );
}
