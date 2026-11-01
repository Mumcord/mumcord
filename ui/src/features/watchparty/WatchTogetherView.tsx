/**
 * The cinema view of the watch party (issue #163, FR-3.124): the 16:9 player as a rounded card in the
 * main column with the channel chat below it, overlay controls that fade in on hover, keyboard focus
 * or pause, the shared keys, and the "Watching" strip of who is in the party under the picture.
 * Leaving the view keeps the party as the mini bar at the top of the chat.
 */
import { useCallback, useEffect, useRef, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { getApi, type User, type WatchCommand } from "../../api";
import { openInBrowser } from "../chat/openLink";
import { useServerStateOf } from "../channels";
import { Avatar } from "../members/Avatar";
import { useAvatars } from "../members/useAvatars";
import { clockTime } from "../media/format";
import { MediaIcon, type MediaIconName } from "../media/icons";
import { MediaFrame } from "../media/MediaFrame";
import { MockPicture } from "../media/MockPicture";
import { mockPlaceholder } from "../media/mockPlaceholder";
import { SeekBar, type SeekMarker } from "../media/SeekBar";
import { ShortcutHint, useFirstUse } from "../media/ShortcutHint";
import { VolumeControl } from "../media/VolumeControl";
import { useMediaView, setFocus } from "../media/mediaView";
import { useMediaKeys } from "../media/useMediaKeys";
import { useFullscreen } from "../media/useFullscreen";
import { useTick } from "../media/useTick";
import { CodecHint } from "./CodecHint";
import { setKnownDuration } from "./knownDuration";
import { PlayPause } from "./PlayPause";
import { SyncPill } from "./SyncPill";
import { loadCommand, playerOrigin } from "./playerBridge";
import { playNext, playPrevious } from "./playlistActions";
import { reportPosition } from "./reportPosition";
import { syncState, type SyncState } from "./syncState";
import { useVideoPreview } from "./useVideoPreview";
import { useWatchPlayer } from "./useWatchPlayer";
import { positionAt, useWatchParty, type LiveParty } from "./watchPartyStore";
import { PLAYING, watchUrl } from "./youtube";
import "./watchParty.css";

/** How often the view recomputes the time and the sync states. */
const TICK_MS = 1000;

/** The icon of the last action chip, so the chip reads at a glance. */
const ACTION_ICON: Record<WatchCommand["kind"], MediaIconName> = {
  play: "play",
  pause: "pause",
  seek: "next",
};

export function WatchTogetherView({ serverId }: { serverId: number }): ReactNode {
  const live = useWatchParty(serverId);
  const view = useMediaView();
  if (live === null || !live.party.joined) return null;
  return <Cinema serverId={serverId} live={live} focus={view.focus} />;
}

/** An icon button in the control row. */
function Icon({
  label,
  icon,
  onClick,
  disabled = false,
}: {
  label: string;
  icon: MediaIconName;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      className="button media-frame__icon"
      aria-label={label}
      data-tooltip={label}
      disabled={disabled}
      onClick={onClick}
    >
      <MediaIcon name={icon} />
    </button>
  );
}

export function Cinema({
  serverId,
  live,
  focus,
}: {
  serverId: number;
  live: LiveParty;
  focus: boolean;
}): ReactNode {
  const { t: tc } = useTranslation("common");
  const { t } = useTranslation("watchparty");
  const party = live.party;
  const { users, selfSession } = useServerStateOf(serverId);
  const frameRef = useRef<HTMLElement | null>(null);
  const embedRef = useRef<HTMLIFrameElement | null>(null);
  const { fullscreen, toggle, exit } = useFullscreen(frameRef);
  const preview = useVideoPreview(party.videoId);
  const title = preview?.title ?? t("playlist.videoTitle");
  const nowMs = useTick(TICK_MS);
  const first = useFirstUse("watchTogether");

  const onCommand = useCallback(
    (command: WatchCommand) => {
      getApi()
        .watchPartyCommand(serverId, command)
        .catch((error: unknown) => {
          console.error(t("commandFailed"), error);
          announce(t("commandFailed"), "assertive");
        });
    },
    [serverId, t],
  );
  const onEnded = useCallback(
    (entry: number) => {
      playNext(serverId, entry, false, t("playlist.nextFailed"));
    },
    [serverId, t],
  );
  const onReport = useCallback(
    (positionMs: number, playing: boolean, buffering: boolean) => {
      reportPosition(serverId, positionMs, playing, buffering);
    },
    [serverId],
  );
  const player = useWatchPlayer(live, { onCommand, onEnded, onReport }, embedRef);
  useEffect(() => {
    setKnownDuration(party.videoId, player.durationMs);
  }, [party.videoId, player.durationMs]);

  useMediaKeys({
    playPause: () => {
      if (player.state === PLAYING) player.pause();
      else player.play();
    },
    seek: (seconds) => {
      player.seekTo(Math.max(0, player.positionMs + seconds * 1000));
    },
    mute: () => {
      player.setVolume(player.volume === 0 ? 1 : 0);
    },
    fullscreen: toggle,
    focus: () => {
      setFocus(!focus);
    },
    escape: () => {
      if (fullscreen) exit();
      else if (focus) setFocus(false);
    },
  });

  const nameOf = (session: number) => users.get(session)?.name ?? t("someone");
  const positionMs = positionAt(live, nowMs);
  const reportOf = (session: number) => party.positions.find((p) => p.session === session);
  const stateOf = (session: number): SyncState => {
    const report = reportOf(session);
    return report === undefined
      ? { kind: "unknown" }
      : syncState(report, positionMs, live.atMs, nowMs);
  };
  const states = party.participants.map(stateOf);
  const markers: SeekMarker[] = party.participants
    .filter((session) => session !== selfSession)
    .flatMap((session) => {
      const report = reportOf(session);
      return report === undefined
        ? []
        : [{ session, positionMs: report.positionMs, name: nameOf(session) }];
    });
  const adder = live.currentAdder;
  const adderName =
    adder === null ? null : (users.get(adder.session)?.name ?? adder.name ?? t("someone"));
  const origin = playerOrigin(player.pageUrl ?? "");
  const action = party.lastAction;
  const time =
    player.durationMs > 0
      ? `${clockTime(player.positionMs / 1000)} / ${clockTime(player.durationMs / 1000)}`
      : clockTime(player.positionMs / 1000);

  return (
    <div className="watch-cinema">
      <MediaFrame
        ref={frameRef}
        label={`${t("title")} – ${title}`}
        paused={!party.playing}
        toolbarLabel={tc("media.controls")}
        hint={first ? <ShortcutHint keys={t("keys")} /> : undefined}
        header={
          <>
            <span className="media-frame__mark">
              <Avatar name={adderName ?? nameOf(party.host)} src={null} size="sm" />
            </span>
            <span className="media-frame__titles">
              <span className="media-frame__title">{title}</span>
              <span className="media-frame__sub">
                {adderName !== null && <span>{t("playlist.addedBy", { name: adderName })}</span>}
                {adderName !== null && <span aria-hidden="true">·</span>}
                <MediaIcon name="crown" size={12} />
                <span>{tc("media.inControl", { name: nameOf(party.host) })}</span>
              </span>
            </span>
            <SyncPill states={states} />
          </>
        }
        picture={
          <div className="media-frame__screen">
            {mockPlaceholder() !== null && <MockPicture label={title} />}
            {origin !== null && mockPlaceholder() === null && !player.failed && (
              <iframe
                key={party.videoId}
                ref={embedRef}
                className="media-frame__frame"
                src={player.pageUrl ?? ""}
                title={t("player")}
                allow="autoplay; encrypted-media; fullscreen; picture-in-picture"
                allowFullScreen
                referrerPolicy="no-referrer"
                sandbox="allow-scripts allow-same-origin allow-presentation"
                data-ready={player.ready}
                onLoad={() => {
                  embedRef.current?.contentWindow?.postMessage(loadCommand(party.videoId), origin);
                }}
              />
            )}
            {player.failed && mockPlaceholder() === null && (
              <div className="media-frame__unavailable" role="status">
                <p>{t("cantPlay")}</p>
                <CodecHint
                  failure={
                    player.youTubeError === null
                      ? null
                      : { kind: "youtube", code: player.youTubeError }
                  }
                  className="watch-party__codec-hint"
                />
                <button
                  type="button"
                  className="button"
                  onClick={() => {
                    openInBrowser(watchUrl(party.videoId));
                  }}
                >
                  {t("openInBrowser")}
                </button>
              </div>
            )}
          </div>
        }
        seek={
          <SeekBar
            label={tc("media.seek")}
            positionMs={player.positionMs}
            durationMs={player.durationMs}
            bufferedMs={player.bufferedMs}
            markers={markers}
            onSeek={(ms) => {
              player.seekTo(ms);
            }}
          />
        }
        controls={
          <>
            <Icon
              label={t("playlist.previous")}
              icon="previous"
              onClick={() => {
                playPrevious(serverId, t("playlist.previousFailed"));
              }}
              disabled={party.history.length === 0}
            />
            <PlayPause
              serverId={serverId}
              live={live}
              className="button media-frame__icon media-frame__icon--main"
            />
            <Icon
              label={tc("media.next")}
              icon="next"
              onClick={() => {
                playNext(serverId, party.currentEntry, true, t("playlist.skipFailed"));
              }}
              disabled={party.playlist.length === 0}
            />
            <VolumeControl
              label={tc("media.volume")}
              level={player.volume}
              onSet={(level) => {
                player.setVolume(level);
              }}
            />
            <span className="media-frame__time">{time}</span>
            <span className="media-frame__action">
              <MediaIcon
                name={action === null ? "play" : ACTION_ICON[action.command.kind]}
                size={12}
              />
              {action === null
                ? tc("media.syncedPlay")
                : t(`other.${action.command.kind}`, { name: nameOf(action.session) })}
            </span>
          </>
        }
        fullscreen={fullscreen}
        onFullscreen={toggle}
        fullscreenLabel={tc("media.fullscreen")}
        exitFullscreenLabel={tc("media.exitFullscreen")}
      />
      {!focus && (
        <WatchingStrip
          serverId={serverId}
          users={users}
          participants={party.participants}
          host={party.host}
          selfSession={selfSession}
          stateOf={stateOf}
        />
      )}
    </div>
  );
}

/** Who is in the party, under the picture: avatar with the sync dot, name, the host's crown. */
function WatchingStrip({
  serverId,
  users,
  participants,
  host,
  selfSession,
  stateOf,
}: {
  serverId: number;
  users: ReadonlyMap<number, User>;
  participants: readonly number[];
  host: number;
  selfSession: number | null;
  stateOf: (session: number) => SyncState;
}): ReactNode {
  const { t: tc } = useTranslation("common");
  const people = participants.flatMap((session) => {
    const user = users.get(session);
    return user ? [user] : [];
  });
  const avatarOf = useAvatars(people, serverId);
  const label = tc("media.watching");
  return (
    <div className="watch-strip">
      <h3 className="watch-strip__label" id={`watch-strip-${String(serverId)}`}>
        {label}
      </h3>
      <ul className="watch-strip__people" aria-labelledby={`watch-strip-${String(serverId)}`}>
        {people.map((person) => {
          const state = stateOf(person.session);
          return (
            <li key={person.session} className="watch-strip__person">
              <span className="watch-list__avatar">
                <Avatar name={person.name} src={avatarOf(person)} size="sm" />
                <span className={`sync-dot sync-dot--${state.kind}`} aria-hidden="true" />
              </span>
              {person.name}
              {person.session === selfSession && (
                <span className="visually-hidden"> ({tc("media.you")})</span>
              )}
              {person.session === host && (
                <span className="watch-strip__host" data-tooltip={tc("media.host")}>
                  <MediaIcon name="crown" size={12} />
                  <span className="visually-hidden">{tc("media.host")}</span>
                </span>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
