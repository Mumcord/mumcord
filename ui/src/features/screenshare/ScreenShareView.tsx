/**
 * The screen share viewer in the main column (issue #163, FR-3.126). The top row names the sharer,
 * what they share and since when, with chips for resolution, frame rate, bitrate, path and sound;
 * "Stream details" opens the popover with the codec, the delay and the dropped frames. The bottom
 * overlay carries the volume of the shared sound with its live level, Fit / Fill / 1:1, Focus, full
 * screen and "Stop watching". The picture figures are counted here, from the frames themselves.
 */
import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { getApi, type P2pLink, type WatchStats } from "../../api";
import { useServerStateOf } from "../channels";
import { VolumeIcon } from "../chat/mediaIcons";
import { Avatar } from "../members/Avatar";
import { useAvatars } from "../members/useAvatars";
import { bitrate, dimension } from "../media/format";
import { MediaFrame } from "../media/MediaFrame";
import { VolumeControl } from "../media/VolumeControl";
import { setFit, useMediaView, type FitMode } from "../media/mediaView";
import { useMediaKeys } from "../media/useMediaKeys";
import { useFullscreen } from "../media/useFullscreen";
import { P2pLinkText } from "../p2p/P2pLinkText";
import { useP2pLink } from "../p2p/useP2pLink";
import { InfoCircleIcon, MonitorIcon, SignalIcon } from "./icons";
import { PathTag, SoundLevel } from "./parts";
import { clockOf } from "./shareFormat";
import { ScreenStage } from "./ScreenStage";
import { useDismiss } from "./useDismiss";
import { shareStartedAt, useScreenShareOf } from "./sharesStore";
import type { StreamStats } from "./streamStats";
import { stopWatching, useWatchedShare, watchMeter } from "./watching";

/** How often the viewer reads its own stream figures and the live sound level (issue #163). */
const POLL_MS = 1000;

export function ScreenShareView(): ReactNode {
  const share = useWatchedShare();
  if (share === null) return null;
  return <Viewer serverId={share.serverId} session={share.session} />;
}

export function Viewer({ serverId, session }: { serverId: number; session: number }): ReactNode {
  const { t, i18n } = useTranslation("screenviewer");
  const share = useScreenShareOf(serverId, session);
  const view = useMediaView();
  const { users } = useServerStateOf(serverId);
  const frameRef = useRef<HTMLElement | null>(null);
  const detailsButton = useRef<HTMLButtonElement>(null);
  const detailsPanel = useRef<HTMLDivElement>(null);
  const meter = watchMeter();
  const { fullscreen, toggle, exit } = useFullscreen(frameRef);
  const link = useP2pLink(serverId, session);
  const [stats, setStats] = useState<WatchStats | null>(null);
  const [details, setDetails] = useState(false);
  const [level, setLevel] = useState(100);
  const user = users.get(session);
  const avatarOf = useAvatars(user ? [user] : [], serverId);
  const name = user?.name ?? t("someone");
  const sound = share?.sound ?? false;
  const paused = share?.paused ?? false;

  const closeDetails = useCallback(() => {
    setDetails(false);
  }, [setDetails]);
  const dismissRefs = useMemo(() => [detailsButton, detailsPanel], []);
  useDismiss(dismissRefs, details, closeDetails);

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
    fitOneToOne: () => {
      setFit("oneToOne");
    },
    escape: () => {
      if (fullscreen) exit();
    },
  });

  const shown = meter.snapshot();
  const what = [
    t(
      share?.kind === "window"
        ? "sharedWindow"
        : share?.kind === "screen"
          ? "sharedScreen"
          : "sharedUnknown",
    ),
    t(sound ? "soundOnShort" : "soundOffShort"),
  ];
  if (share !== null) {
    what.push(t("since", { time: clockOf(shareStartedAt(share), i18n.language) }));
  }

  return (
    <MediaFrame
      ref={frameRef}
      label={t("label", { name })}
      paused={paused}
      toolbarLabel={t("controls")}
      header={
        <>
          <span className="media-frame__mark">
            <Avatar name={name} src={user === undefined ? null : avatarOf(user)} size="sm" live />
          </span>
          <span className="media-frame__titles">
            <span className="media-frame__title">{t("title", { name })}</span>
            <span className="media-frame__sub">{what.join(" · ")}</span>
          </span>
          <span className="live-badge live-badge--large" data-paused={paused || undefined}>
            {t(paused ? "pausedShort" : "live")}
          </span>
          <Chips
            t={t}
            shown={shown}
            link={link}
            sound={sound}
            details={details}
            detailsButton={detailsButton}
            onDetails={() => {
              setDetails((open) => !open);
            }}
          />
        </>
      }
      picture={
        <div className={`share-screen share-screen--${view.fit}`} data-fit={view.fit}>
          <ScreenStage
            serverId={serverId}
            session={session}
            name={name}
            paused={paused}
            meter={meter}
          />
          {details && <StreamDetails t={t} shown={shown} link={link} panelRef={detailsPanel} />}
        </div>
      }
      controls={
        <>
          {sound && (
            <>
              <VolumeControl
                label={t("volume")}
                level={level / 100}
                onSet={(fraction) => {
                  setVolume(Math.round(fraction * 100));
                }}
              />
              <SoundLevel label={t("soundOf", { name })} rms={stats?.rms ?? null} />
            </>
          )}
          <FitButtons t={t} fit={view.fit} />
          <button
            type="button"
            className="button share-stop"
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

/** The top row chips: resolution, frame rate, bitrate, path, sound and the stream details. */
function Chips({
  t,
  shown,
  link,
  sound,
  details,
  detailsButton,
  onDetails,
}: {
  t: TFunction<"screenviewer">;
  shown: StreamStats;
  link: P2pLink | null;
  sound: boolean;
  details: boolean;
  detailsButton: RefObject<HTMLButtonElement | null>;
  onDetails: () => void;
}): ReactNode {
  // Before the first frame there is nothing to count yet; the chips appear with the picture.
  const live = shown.width > 0;
  return (
    <span className="share-chips">
      {live && (
        <>
          <span className="share-chip" data-tooltip={t("resolution")}>
            <MonitorIcon />
            {dimension(shown.width, shown.height)}
          </span>
          <span className="share-chip" data-tooltip={t("frameRate")}>
            {`${String(shown.fps)} fps`}
          </span>
          {shown.kbps !== null && (
            <span className="share-chip" data-tooltip={t("bitrate")}>
              <SignalIcon />
              {bitrate(shown.kbps)}
            </span>
          )}
        </>
      )}
      {link !== null && <PathTag path={link.path} chip />}
      <span className="share-chip" data-tooltip={t("sound")}>
        <VolumeIcon muted={!sound} size={14} />
        {t(sound ? "soundOn" : "soundOff")}
      </span>
      <button
        ref={detailsButton}
        type="button"
        className="share-chip share-chip--button"
        aria-expanded={details}
        aria-label={t("streamDetails")}
        data-tooltip={t("streamDetails")}
        onClick={onDetails}
      >
        <InfoCircleIcon />
      </button>
    </span>
  );
}

/** Fit / Fill / 1:1 for the picture, as one segmented control. */
function FitButtons({ t, fit }: { t: TFunction<"screenviewer">; fit: FitMode }): ReactNode {
  return (
    <span className="share-segments" role="group" aria-label={t("fitGroup")}>
      {(["fit", "fill", "oneToOne"] as const).map((mode) => (
        <button
          key={mode}
          type="button"
          className="share-segments__button"
          aria-pressed={mode === fit}
          aria-keyshortcuts={mode === "oneToOne" ? "1" : undefined}
          onClick={() => {
            setFit(mode);
          }}
        >
          {t(mode)}
        </button>
      ))}
    </span>
  );
}

/** Codec, delay and dropped frames: why the picture looks the way it does (issue #163). */
function StreamDetails({
  t,
  shown,
  link,
  panelRef,
}: {
  t: TFunction<"screenviewer">;
  shown: StreamStats;
  link: P2pLink | null;
  panelRef: RefObject<HTMLDivElement | null>;
}): ReactNode {
  const { i18n } = useTranslation();
  const titleId = useId();
  const unknown = "—";
  const live = shown.width > 0;
  const rows: [string, ReactNode][] = [
    [t("resolution"), live ? dimension(shown.width, shown.height) : unknown],
    [t("frameRate"), live ? `${String(shown.fps)} fps` : unknown],
    [t("bitrate"), shown.kbps === null ? unknown : bitrate(shown.kbps)],
    [t("codec"), "VP9 · Opus"],
    [t("path"), link === null ? unknown : <P2pLinkText link={link} />],
    [t("delay"), shown.delayMs === null ? unknown : `${String(shown.delayMs)} ms`],
    [
      t("droppedFrames"),
      shown.droppedShare === null
        ? unknown
        : new Intl.NumberFormat(i18n.language, {
            style: "percent",
            maximumFractionDigits: 1,
          }).format(shown.droppedShare),
    ],
  ];
  return (
    <div ref={panelRef} className="share-details" role="group" aria-labelledby={titleId}>
      <p id={titleId} className="share-details__title">
        <InfoCircleIcon />
        {t("streamDetails")}
      </p>
      <dl className="share-details__list">
        {rows.map(([label, value]) => (
          <div key={label} className="share-details__row">
            <dt>{label}</dt>
            <dd>{value}</dd>
          </div>
        ))}
      </dl>
    </div>
  );
}
