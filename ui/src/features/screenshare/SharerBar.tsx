/**
 * The sharer's control bar (issue #163, FR-3.127): a compact row at the top of the chat while the
 * user shares — LIVE, how long it runs, what is shared and the quality it sends, with Pause, Sound,
 * Change window, Viewers (each viewer's frame rate and path, and the hint when quality is lowered for
 * one of them) and Stop. It replaces the red bar in the sidebar and the controls in the user panel.
 */
import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { SharePanel } from "./SharePanel";
import { getApi, type ShareQuality, type User } from "../../api";
import { useServerStateOf } from "../channels";
import { Avatar } from "../members/Avatar";
import { useAvatars } from "../members/useAvatars";
import { useTick } from "../media/useTick";
import { MAX_VIEWERS, Viewers } from "./Viewers";
import { useOwnShare } from "./useOwnShare";
import { useShareActions } from "./useShareActions";

/** Viewers per sharer (FR-2.43); the ninth gets a "share is full" answer. */

/** The quality the sharer sends is read once a second (issue #163). */
const POLL_MS = 1000;

export function SharerBar(): ReactNode {
  const { t } = useTranslation("screenshare");
  const { t: tv } = useTranslation("screenviewer");
  const { t: tc } = useTranslation("common");
  const share = useOwnShare();
  const { start, stop } = useShareActions(share);
  const serverId = share.serverId;
  const own = share.own;
  const { users, selfSession } = useServerStateOf(serverId);
  const people: User[] = [...users.values()];
  const avatarOf = useAvatars(people, serverId ?? undefined);
  const [quality, setQuality] = useState<ShareQuality | null>(null);
  const [open, setOpen] = useState(false);
  useTick(POLL_MS);

  useEffect(() => {
    if (serverId === null || own === null) return;
    const api = getApi();
    let current = true;
    const read = () => {
      api
        .screenShareQuality(serverId)
        .then((q) => {
          if (current) setQuality(q);
        })
        .catch((error: unknown) => {
          console.warn("screen share quality failed", error);
        });
    };
    read();
    const timer = window.setInterval(read, POLL_MS);
    return () => {
      current = false;
      window.clearInterval(timer);
    };
  }, [serverId, own]);

  // Nothing shared yet: the bar is the entry point, with the reason if sharing is not possible.
  if (own === null) {
    return (
      <div className="sharer-bar" role="status" aria-label={t("controls")}>
        <SharePanel />
      </div>
    );
  }
  if (serverId === null) return null;
  const nameOf = (session: number) => users.get(session)?.name ?? t("someone");
  const srcOf = (session: number) => {
    const user = users.get(session);
    return user === undefined ? null : avatarOf(user);
  };
  const slow = quality?.viewers.find((viewer) => viewer.struggling) ?? null;
  const names = own.viewers.map(nameOf).join(", ");
  const full = own.viewers.length >= MAX_VIEWERS;

  const run = (call: Promise<null>, what: string) => {
    call.catch((error: unknown) => {
      console.error(`changing the screen share failed (${what})`, error);
      announce(t("changeFailed"), "assertive");
    });
  };

  // The system picker only opens with a fresh share: end this one and choose again.
  const changeWindow = () => {
    stop();
    void start();
  };

  return (
    <div className="sharer-bar" role="status" aria-label={t("controls")}>
      <span className="live-badge">{t("live")}</span>
      <span className="sharer-bar__time">
        {t("elapsed", { minutes: String(Math.max(1, Math.round(own.elapsedMs / 60000))) })}
      </span>
      <span className="sharer-bar__source">
        {t("sharingBar")} ·{" "}
        {tv(
          own.kind === "window"
            ? "sharedWindow"
            : own.kind === "screen"
              ? "sharedScreen"
              : "sharedUnknown",
        )}
        {quality === null ? "" : ` · ${String(quality.fps)} fps`}
      </span>
      {own.paused && <span className="sharer-bar__paused">{t("paused")}</span>}
      {!own.soundAvailable && (
        <span className="sharer-bar__hint">
          {t(share.platform?.sound === false ? "noSound" : "noSoundForShare")}
        </span>
      )}
      <button
        type="button"
        className="button"
        aria-pressed={own.paused}
        onClick={() => {
          run(getApi().screenShareSetPaused(serverId, !own.paused), "pause");
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
            run(getApi().screenShareSetSound(serverId, !own.sound), "sound");
          }}
        >
          {t("sound")}
        </button>
      )}
      <button type="button" className="button" onClick={changeWindow}>
        {t("changeWindow")}
      </button>
      <Viewers serverId={serverId} own={own} />
      <button
        type="button"
        className="button"
        aria-haspopup="true"
        aria-expanded={open}
        aria-label={t("viewersTitle")}
        onClick={() => {
          setOpen((value) => !value);
        }}
      >
        <span aria-hidden="true">⋯</span>
      </button>
      {open && (
        <div className="sharer-bar__popover" role="group" aria-label={t("viewersTitle")}>
          {full && <p>{t("full", { max: MAX_VIEWERS, names })}</p>}
          {own.viewers.length === 0 && <p>{t("viewersNone")}</p>}
          {own.viewers.map((session) => {
            const viewer = quality?.viewers.find((v) => v.session === session);
            return (
              <p key={session} className="sharer-bar__viewer">
                <Avatar name={nameOf(session)} src={srcOf(session)} size="sm" />
                <span className="sharer-bar__name">
                  {nameOf(session)}
                  {session === selfSession && ` (${tc("media.you")})`}
                </span>
                <span className="sharer-bar__quality">
                  {viewer === undefined
                    ? tv("unknown")
                    : `${String(viewer.fps)} fps, ${tv("pathDirect")}`}
                </span>
              </p>
            );
          })}
          {slow !== null && (
            <p className="sharer-bar__hint">
              {tv("loweredFor", { name: nameOf(slow.session), fps: String(slow.fps) })}
            </p>
          )}
        </div>
      )}
      <button
        type="button"
        className="button button--danger"
        onClick={stop}
      >
        {t("stop")}
      </button>
    </div>
  );
}
