/**
 * The screen share viewer in the main column (issue #163, FR-3.126). The header names the sharer,
 * what they share and since when, with chips for resolution, frame rate, bitrate, path and sound;
 * "Stream details" opens the popover with the codec, the delay and the dropped frames. The bottom
 * overlay carries the volume of the shared sound with its live level, Fit / Fill / 1:1, Focus, full
 * screen and "Stop watching". The picture figures are counted here, from the frames themselves.
 */
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { getApi, type WatchStats } from "../../api";
import { useServerStateOf } from "../channels";
import { Avatar } from "../members/Avatar";
import { useAvatars } from "../members/useAvatars";
import { bitrate, dimension } from "../media/format";
import { MediaFrame } from "../media/MediaFrame";
import { VolumeControl } from "../media/VolumeControl";
import { mediaView, setFit, setFocus, useMediaView, type FitMode } from "../media/mediaView";
import { useMediaKeys } from "../media/useMediaKeys";
import { useFullscreen } from "../media/useFullscreen";
import { useFirstUse, ShortcutHint } from "../media/ShortcutHint";
import { ScreenStage } from "./ScreenStage";
import { useScreenShareOf } from "./sharesStore";
import { StreamMeter, type StreamStats } from "./streamStats";
import { stopWatching, useWatchedShare } from "./watching";

/** How often the viewer reads its own stream figures and the live sound level (issue #163). */
const POLL_MS = 1000;

export function ScreenShareView(): ReactNode {
  const share = useWatchedShare();
  if (share === null) return null;
  return <Viewer serverId={share.serverId} session={share.session} />;
}

export function Viewer({ serverId, session }: { serverId: number; session: number }): ReactNode {
  const { t: tc } = useTranslation("common");
  const { t } = useTranslation("screenviewer");
  const share = useScreenShareOf(serverId, session);
  const view = useMediaView();
  const { users } = useServerStateOf(serverId);
  const frameRef = useRef<HTMLElement | null>(null);
  const meter = useMemo(() => new StreamMeter(), []);
  const { fullscreen, toggle, exit } = useFullscreen(frameRef);
  const first = useFirstUse("screenShare");
  const [stats, setStats] = useState<WatchStats | null>(null);
  const [details, setDetails] = useState(false);
  const [level, setLevel] = useState(100);
  const user = users.get(session);
  const avatarOf = useAvatars(user ? [user] : [], serverId);
  const name = user?.name ?? t("someone");

  useEffect(() => {
    const api = getApi();
    let current = true;
    const poll = () => {
      api
        .screenShareWatchStats(serverId, session)
        .then((watch) => {
          if (current) setStats(watch);
        })
        .catch((error: unknown) => {
          console.warn("screen share watch stats failed", error);
        });
    };
    poll();
    const timer = window.setInterval(poll, POLL_MS);
    return () => {
      current = false;
      window.clearInterval(timer);
    };
  }, [serverId, session]);

  const setVolume = (percent: number) => {
    setLevel(percent);
    getApi()
      .personSetVolume(serverId, session, percent)
      .catch((error: unknown) => {
        console.warn("setting the volume of the shared sound failed", error);
      });
  };

  useMediaKeys({
    mute: () => {
      setVolume(level === 0 ? 100 : 0);
    },
    fullscreen: toggle,
    focus: () => {
      if (mediaView().cinema === "share") setFocus(!view.focus);
    },
    fitOneToOne: () => {
      setFit("oneToOne");
    },
    escape: () => {
      if (fullscreen) exit();
      else if (view.focus) setFocus(false);
    },
  });

  const shown = meter.snapshot();
  return (
    <MediaFrame
      ref={frameRef}
      label={t("label", { name })}
      paused={share?.paused ?? false}
      toolbarLabel={t("controls")}
      hint={first ? <ShortcutHint keys={tc("media.keys")} /> : undefined}
      header={
        <>
          <span className="media-frame__mark">
            <Avatar name={name} src={user === undefined ? null : avatarOf(user)} size="sm" live />
          </span>
          <span className="media-frame__titles">
            <span className="media-frame__title">{t("title", { name })}</span>
            <span className="media-frame__sub">
              {what(t, share?.kind ?? null, share?.elapsedMs ?? 0, share?.sound ?? false)}
            </span>
          </span>
          <span className="media-frame__live">{t("live")}</span>
          <Chips t={t} shown={shown} path={stats?.path ?? null} sound={share?.sound ?? false} />
        </>
      }
      picture={
        <div className={`media-frame__screen media-frame__screen--${view.fit}`} data-fit={view.fit}>
          <ScreenStage
            serverId={serverId}
            session={session}
            name={name}
            paused={share?.paused ?? false}
            meter={meter}
          />
          {details && <StreamDetails t={t} shown={shown} path={stats?.path ?? null} />}
        </div>
      }
      controls={
        <>
          <VolumeControl
            label={t("volume")}
            level={level / 100}
            meter={stats === null ? null : stats.rms}
            onSet={(fraction) => {
              setVolume(Math.round(fraction * 100));
            }}
          />
          <FitButtons t={t} fit={view.fit} />
          <button
            type="button"
            className="button"
            aria-pressed={details}
            data-tooltip={t("streamDetails")}
            onClick={() => {
              setDetails((open) => !open);
            }}
          >
            {t("streamDetails")}
          </button>
          <button
            type="button"
            className="button"
            aria-pressed={view.focus}
            aria-keyshortcuts="Z"
            data-tooltip={t("focus")}
            onClick={() => {
              setFocus(!view.focus);
            }}
          >
            {t("focus")}
          </button>
          <button
            type="button"
            className="button"
            onClick={() => {
              stopWatching();
              setDetails(false);
            }}
          >
            {t("stop")}
          </button>
        </>
      }
      fullscreen={fullscreen}
      onFullscreen={toggle}
      fullscreenLabel={t("fullscreen")}
      exitFullscreenLabel={t("exitFullscreen")}
    />
  );
}

/** "Window · sound · since 12 min", or what the platform cannot tell as "screen". */
function what(
  t: TFunction<"screenviewer">,
  kind: string | null,
  elapsedMs: number,
  sound: boolean,
): string {
  const parts = [
    t(kind === "window" ? "sharedWindow" : kind === "screen" ? "sharedScreen" : "sharedUnknown"),
  ];
  parts.push(t(sound ? "soundOn" : "soundOff"));
  parts.push(t("since", { minutes: Math.max(1, Math.round(elapsedMs / 60000)) }));
  return parts.join(" · ");
}

/** The header chips: resolution, frame rate, bitrate, path and sound. */
function Chips({
  t,
  shown,
  path,
  sound,
}: {
  t: TFunction<"screenviewer">;
  shown: StreamStats;
  path: string | null;
  sound: boolean;
}): ReactNode {
  return (
    <span className="media-frame__chips">
      <span
        data-tooltip={t("resolution")}
      >{dimension(shown.width, shown.height)}</span>
      <span data-tooltip={t("frameRate")}>{`${String(shown.fps)} fps`}</span>
      <span data-tooltip={t("bitrate")}>{bitrate(shown.kbps)}</span>
      <span data-tooltip={t("path")}>{t(path === null ? "pathUnknown" : "pathDirect")}</span>
      <span data-tooltip={t("sound")}>{t(sound ? "soundOn" : "soundOff")}</span>
    </span>
  );
}

/** Fit / Fill / 1:1 for the picture. */
function FitButtons({ t, fit }: { t: TFunction<"screenviewer">; fit: FitMode }): ReactNode {
  return (
    <span className="media-frame__fit" role="group" aria-label={t("fitGroup")}>
      {(["fit", "fill", "oneToOne"] as const).map((mode) => (
        <button
          key={mode}
          type="button"
          className="button"
          aria-pressed={mode === fit}
          onClick={() => {
            setFit(mode);
          }}
        >
          {t(mode === "oneToOne" ? "oneToOne" : mode)}
        </button>
      ))}
    </span>
  );
}

/** Codec, delay and dropped frames: why the picture looks the way it does (issue #163). */
function StreamDetails({
  t,
  shown,
  path,
}: {
  t: TFunction<"screenviewer">;
  shown: StreamStats;
  path: string | null;
}): ReactNode {
  return (
    <div className="stream-details" role="group" aria-label={t("streamDetails")}>
      <p>
        <span>{t("resolution")}</span>
        <span>{dimension(shown.width, shown.height)}</span>
      </p>
      <p>
        <span>{t("frameRate")}</span>
        <span>{`${String(shown.fps)} fps`}</span>
      </p>
      <p>
        <span>{t("bitrate")}</span>
        <span>{bitrate(shown.kbps)}</span>
      </p>
      <p>
        <span>{t("codec")}</span>
        <span>VP9, Opus</span>
      </p>
      <p>
        <span>{t("path")}</span>
        <span>{t(path === null ? "pathUnknown" : "pathDirect")}</span>
      </p>
      <p>
        <span>{t("delay")}</span>
        <span>{shown.delayMs === null ? "—" : `${String(shown.delayMs)} ms`}</span>
      </p>
      <p>
        <span>{t("droppedFrames")}</span>
        <span>{String(shown.dropped)}</span>
      </p>
    </div>
  );
}
