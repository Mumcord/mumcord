/**
 * The sharer's control bar (issue #163, FR-3.127): a compact card at the top of the chat while the
 * user shares — the live picture of what is shared, LIVE, how long it runs, what is shared and the
 * quality it sends, with Pause, Sound, Change window, Viewers (each viewer's quality and path, and
 * the hint when quality is lowered for one of them) and Stop. It replaces the red bar in the sidebar
 * and the controls in the user panel.
 */
import { useCallback, useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { getApi, type ShareQuality, type ShareThumbnail, type User } from "../../api";
import { useServerStateOf } from "../channels";
import { PauseIcon, PlayIcon, StopIcon, VolumeIcon } from "../chat/mediaIcons";
import { Avatar } from "../members/Avatar";
import { useAvatars } from "../members/useAvatars";
import { clockTime } from "../media/format";
import { useTick } from "../media/useTick";
import { useP2pLink } from "../p2p/useP2pLink";
import { ChevronDownIcon, InfoCircleIcon, WindowIcon } from "./icons";
import { PathTag } from "./parts";
import { qualityText } from "./shareFormat";
import { shareStartedAt } from "./sharesStore";
import { useOwnShare } from "./useOwnShare";
import { useDismiss } from "./useDismiss";
import { useShareActions } from "./useShareActions";
import { MAX_VIEWERS } from "./Viewers";

/** The quality the sharer sends is read once a second (issue #163). */
const POLL_MS = 1000;
/** Avatars on the viewers button; the count says the rest. */
const STACKED = 3;

export function SharerBar(): ReactNode {
  const { t } = useTranslation("screenshare");
  const { t: tv } = useTranslation("screenviewer");
  const share = useOwnShare();
  const { start, stop } = useShareActions(share);
  const serverId = share.serverId;
  const own = share.own;
  const { users } = useServerStateOf(serverId);
  const people: User[] = [...users.values()];
  const avatarOf = useAvatars(people, serverId ?? undefined);
  const [quality, setQuality] = useState<ShareQuality | null>(null);
  const [thumb, setThumb] = useState<ShareThumbnail | null>(null);
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const popover = useRef<HTMLDivElement>(null);
  const popoverId = useId();
  const noSoundId = useId();
  const now = useTick(POLL_MS);
  const close = useCallback(() => {
    setOpen(false);
  }, []);
  const dismissRefs = useMemo(() => [button, popover], []);
  useDismiss(dismissRefs, open, close);

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
      api
        .screenShareThumbnail(serverId)
        .then((picture) => {
          if (current) setThumb(picture);
        })
        .catch((error: unknown) => {
          console.warn("screen share thumbnail failed", error);
        });
    };
    read();
    const timer = window.setInterval(read, POLL_MS);
    return () => {
      current = false;
      window.clearInterval(timer);
    };
  }, [serverId, own]);

  // Starting a share lives in the composer's Apps menu, which also names what blocks it.
  if (own === null || serverId === null) return null;
  const nameOf = (session: number) => users.get(session)?.name ?? t("someone");
  const srcOf = (session: number) => {
    const user = users.get(session);
    return user === undefined ? null : avatarOf(user);
  };
  const slow = quality?.viewers.find((viewer) => viewer.struggling) ?? null;
  const count = own.viewers.length;

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

  const source = [
    tv(
      own.kind === "window"
        ? "sharedWindow"
        : own.kind === "screen"
          ? "sharedScreen"
          : "sharedUnknown",
    ),
  ];
  if (quality !== null && quality.height > 0) source.push(qualityText(quality.height, quality.fps));
  source.push(tv(own.sound ? "soundOnShort" : "soundOffShort"));
  const noSound = t(share.platform?.sound === false ? "noSound" : "noSoundForShare");

  return (
    <div className="sharer-bar" role="group" aria-label={t("controls")}>
      <span className="sharer-bar__thumb">
        {thumb !== null && (
          <img src={thumb.dataUrl} alt={t("thumbnail")} width={thumb.width} height={thumb.height} />
        )}
      </span>
      <span className="sharer-bar__info">
        <span className="sharer-bar__line">
          <span className="live-badge live-badge--large" data-paused={own.paused || undefined}>
            {own.paused ? t("paused") : t("live")}
          </span>
          <span className="sharer-bar__title">{t("youAreSharing")}</span>
          <span className="sharer-bar__time">{clockTime((now - shareStartedAt(own)) / 1000)}</span>
        </span>
        <span className="sharer-bar__source">{source.join(" · ")}</span>
      </span>
      <span className="sharer-bar__actions">
        <button
          type="button"
          className="button sharer-bar__button"
          aria-pressed={own.paused}
          onClick={() => {
            run(getApi().screenShareSetPaused(serverId, !own.paused), "pause");
          }}
        >
          {own.paused ? <PlayIcon size={14} /> : <PauseIcon size={14} />}
          {own.paused ? t("resume") : t("pause")}
        </button>
        <button
          type="button"
          className="button sharer-bar__button"
          aria-pressed={own.soundAvailable ? own.sound : undefined}
          aria-disabled={!own.soundAvailable || undefined}
          aria-describedby={own.soundAvailable ? undefined : noSoundId}
          data-tooltip={own.soundAvailable ? t("sound") : noSound}
          onClick={() => {
            if (own.soundAvailable)
              run(getApi().screenShareSetSound(serverId, !own.sound), "sound");
          }}
        >
          <VolumeIcon muted={!own.sound} size={14} />
          {tv("sound")}
        </button>
        {!own.soundAvailable && (
          <span id={noSoundId} className="visually-hidden">
            {noSound}
          </span>
        )}
        <button type="button" className="button sharer-bar__button" onClick={changeWindow}>
          <WindowIcon />
          {t("changeWindow")}
        </button>
        <span className="sharer-bar__viewers">
          <button
            ref={button}
            type="button"
            className="button sharer-bar__button sharer-bar__viewers-button"
            aria-expanded={open}
            aria-controls={open ? popoverId : undefined}
            onClick={() => {
              setOpen((value) => !value);
            }}
          >
            {count > 0 && (
              <span className="sharer-bar__stack" aria-hidden="true">
                {own.viewers.slice(0, STACKED).map((session) => (
                  <Avatar key={session} name={nameOf(session)} src={srcOf(session)} size="sm" />
                ))}
              </span>
            )}
            {t("viewerCount", { count })}
            <ChevronDownIcon />
          </button>
          {open && (
            <div
              ref={popover}
              id={popoverId}
              className="sharer-bar__popover"
              role="group"
              aria-label={t("viewersTitle")}
            >
              <p className="sharer-bar__popover-title">
                {t("viewersTitle")}{" "}
                <span className="sharer-bar__popover-count">
                  {t("ofMax", { count, max: MAX_VIEWERS })}
                </span>
              </p>
              {count === 0 ? (
                <p className="sharer-bar__empty">{t("viewersNone")}</p>
              ) : (
                <ul className="viewer-panel__list">
                  {own.viewers.map((session) => {
                    const viewer = quality?.viewers.find((v) => v.session === session);
                    return (
                      <ViewerRow
                        key={session}
                        serverId={serverId}
                        session={session}
                        name={nameOf(session)}
                        src={srcOf(session)}
                        quality={
                          viewer === undefined || quality === null
                            ? null
                            : qualityText(quality.height, viewer.fps)
                        }
                      />
                    );
                  })}
                </ul>
              )}
              {count >= MAX_VIEWERS && <p className="sharer-bar__note">{t("fullShort")}</p>}
              {slow !== null && (
                <p className="sharer-bar__note">
                  <InfoCircleIcon />
                  {tv("loweredFor", { name: nameOf(slow.session), fps: String(slow.fps) })}
                </p>
              )}
            </div>
          )}
        </span>
        <button
          type="button"
          className="button sharer-bar__button sharer-bar__stop"
          aria-label={t("stop")}
          data-tooltip={t("stop")}
          onClick={stop}
        >
          <StopIcon size={12} />
          {t("stopShort")}
        </button>
      </span>
    </div>
  );
}

/** One viewer in the popover: who, the quality they get and how the picture reaches them. */
function ViewerRow({
  serverId,
  session,
  name,
  src,
  quality,
}: {
  serverId: number;
  session: number;
  name: string;
  src: string | null;
  quality: string | null;
}): ReactNode {
  const link = useP2pLink(serverId, session);
  return (
    <li className="viewer-panel__person">
      <Avatar name={name} src={src} />
      <span className="viewer-panel__text">
        <span className="viewer-panel__name">{name}</span>
        {quality !== null && <span className="viewer-panel__sub">{quality}</span>}
      </span>
      {link !== null && <PathTag path={link.path} />}
    </li>
  );
}
