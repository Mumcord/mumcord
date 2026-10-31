/**
 * The cinema view of the watch party (issue #163, FR-3.124): the 16:9 player in the main column with
 * the channel chat below it, overlay controls that fade in on hover, keyboard focus or pause, and the
 * shared keys. Leaving the view keeps the party as the mini bar at the top of the chat.
 */
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { getApi, type WatchCommand } from "../../api";
import { openInBrowser } from "../chat/openLink";
import { safeImage } from "../chat/links";
import { useServerStateOf } from "../channels";
import { Avatar } from "../members/Avatar";
import { clockTime } from "../media/format";
import { MediaFrame } from "../media/MediaFrame";
import { MockPicture } from "../media/MockPicture";
import { mockPlaceholder } from "../media/mockPlaceholder";
import { SeekBar, type SeekMarker } from "../media/SeekBar";
import { ShortcutHint, useFirstUse } from "../media/ShortcutHint";
import { VolumeControl } from "../media/VolumeControl";
import { useMediaView, setFocus } from "../media/mediaView";
import { useMediaKeys } from "../media/useMediaKeys";
import { useFullscreen } from "../media/useFullscreen";
import { CodecHint } from "./CodecHint";
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

/** How often the view recomputes the time and the sync states. */
const TICK_MS = 1000;

export function WatchTogetherView({ serverId }: { serverId: number }): ReactNode {
  const live = useWatchParty(serverId);
  const view = useMediaView();
  if (live === null || !live.party.joined) return null;
  return <Cinema serverId={serverId} live={live} focus={view.focus} />;
}

function useTick(ms: number): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => {
      setNow(Date.now());
    }, ms);
    return () => {
      window.clearInterval(timer);
    };
  }, [ms]);
  return now;
}

/** An icon button in the control row. */
function Icon({
  label,
  glyph,
  onClick,
  disabled = false,
}: {
  label: string;
  glyph: string;
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
      <span aria-hidden="true">{glyph}</span>
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
  const states: SyncState[] = party.participants.map((session) => {
    const report = reportOf(session);
    return report === undefined
      ? { kind: "unknown" }
      : syncState(report, positionMs, live.atMs, nowMs);
  });
  const markers: SeekMarker[] = party.participants
    .filter((session) => session !== selfSession)
    .flatMap((session) => {
      const report = reportOf(session);
      return report === undefined
        ? []
        : [{ session, positionMs: report.positionMs, name: nameOf(session) }];
    });
  const adder = party.playlist.find((entry) => entry.id === party.currentEntry);
  const control = party.lastAction?.session ?? party.skippedBy ?? null;
  const origin = playerOrigin(player.pageUrl ?? "");

  return (
    <MediaFrame
      ref={frameRef}
      label={`${t("title")} – ${title}`}
      paused={!party.playing}
      toolbarLabel={tc("media.controls")}
      hint={first ? <ShortcutHint keys={tc("media.keys")} /> : undefined}
      header={
        <>
          <span className="media-frame__mark">
            {preview?.image != null && safeImage(preview.image) !== null ? (
              <img src={safeImage(preview.image) as string} alt="" />
            ) : (
              <Avatar name={nameOf(party.host)} src={null} size="sm" />
            )}
          </span>
          <span className="media-frame__titles">
            <span className="media-frame__title">{title}</span>
            <span className="media-frame__sub">
              {adder !== undefined
                ? t("playlist.addedBy", { name: nameOf(adder.addedBy) })
                : t("someone")}
              {control !== null && ` · ${tc("media.inControl", { name: nameOf(control) })}`}
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
      controls={
        <>
          <Icon
            label={t("playlist.previous")}
            glyph="⏮"
            onClick={() => {
              playPrevious(serverId, t("playlist.previousFailed"));
            }}
            disabled={party.history.length === 0}
          />
          <PlayPause serverId={serverId} live={live} />
          <Icon
            label={tc("media.next")}
            glyph="⏭"
            onClick={() => {
              playNext(serverId, party.currentEntry, true, t("playlist.skipFailed"));
            }}
            disabled={party.playlist.length === 0}
          />
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
          <VolumeControl
            label={tc("media.volume")}
            level={player.volume}
            onSet={(level) => {
              player.setVolume(level);
            }}
          />
          <span className="media-frame__time">{clockTime(player.positionMs / 1000)}</span>
          <span className="media-frame__action" data-tooltip={tc("media.syncedPlay")}>
            {party.lastAction === null
              ? tc("media.syncedPlay")
              : t(`other.${party.lastAction.command.kind}`, {
                  name: nameOf(party.lastAction.session),
                })}
          </span>
        </>
      }
      fullscreen={fullscreen}
      onFullscreen={toggle}
      fullscreenLabel={tc("media.fullscreen")}
      exitFullscreenLabel={tc("media.exitFullscreen")}
    />
  );
}
