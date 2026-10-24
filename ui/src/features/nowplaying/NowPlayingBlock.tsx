/**
 * "Now playing" block for a person's profile card and preview (FR-1.58): cover (loaded through
 * the engine, only while link previews are on), title, artist and a progress bar that advances
 * with the local clock between updates.
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type NowPlaying } from "../../api";
import "./nowPlaying.css";
import { formatTime, positionAt, type LiveTrack } from "./progress";
import { useNowPlaying } from "./nowPlayingStore";

/** How often the progress bar moves on while playing. */
export const PROGRESS_TICK_MS = 1000;

/** The block for `session`, or nothing while the person shares no track. */
export function NowPlayingFor({
  session,
  compact = false,
}: {
  session: number;
  compact?: boolean;
}) {
  const live = useNowPlaying(session);
  return live ? <NowPlayingBlock session={session} live={live} compact={compact} /> : null;
}

export function NowPlayingBlock({
  session,
  live,
  compact = false,
}: {
  session: number;
  live: LiveTrack;
  compact?: boolean;
}) {
  const { t } = useTranslation("members");
  const { track } = live;
  const cover = useCover(session, track);
  const now = useClock(track.playing);
  const position = positionAt(live, now);
  const className = compact ? "now-playing now-playing--compact" : "now-playing";
  return (
    <section className={className} aria-label={t("nowPlaying.label")}>
      {cover !== null && <img className="now-playing__cover" src={cover} alt="" />}
      <div className="now-playing__text">
        <p className="now-playing__state">
          {track.playing ? t("nowPlaying.listening") : t("nowPlaying.paused")}
        </p>
        <p className="now-playing__title">{track.title}</p>
        {track.artist && <p className="now-playing__artist">{track.artist}</p>}
        {track.durationMs !== null && (
          <div className="now-playing__progress">
            <progress
              max={track.durationMs}
              value={position}
              aria-label={t("nowPlaying.progress")}
              aria-valuetext={t("nowPlaying.position", {
                position: formatTime(position),
                duration: formatTime(track.durationMs),
              })}
            />
            <span className="now-playing__time" aria-hidden="true">
              {formatTime(position)} / {formatTime(track.durationMs)}
            </span>
          </div>
        )}
      </div>
    </section>
  );
}

/** Current time, refreshed every second while `running`. */
function useClock(running: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    // While paused the position ignores the clock; a resumed track arrives with a new anchor.
    if (!running) return;
    const timer = setInterval(() => {
      setNow(Date.now());
    }, PROGRESS_TICK_MS);
    return () => {
      clearInterval(timer);
    };
  }, [running]);
  return now;
}

/** The cover as a `data:` URL from the engine; reloaded when the track changes. */
function useCover(session: number, track: NowPlaying): string | null {
  const { hasCover, title, artist } = track;
  const key = `${String(session)}\n${artist}\n${title}`;
  const [loaded, setLoaded] = useState<{ key: string; cover: string | null } | null>(null);
  useEffect(() => {
    if (!hasCover) return;
    let active = true;
    getApi()
      .nowPlayingCover(session)
      .then(
        (cover) => {
          if (active) setLoaded({ key, cover });
        },
        (e: unknown) => {
          console.error("Loading a cover failed", e);
        },
      );
    return () => {
      active = false;
    };
  }, [session, hasCover, key]);
  return hasCover && loaded?.key === key ? loaded.cover : null;
}
