/**
 * "Now gaming" block for a person's profile card and preview (FR-1.59): game icon (loaded through
 * the engine from the Steam CDN, only while link previews are on; a generic controller otherwise),
 * game name and play time. The full card offers "Join game" when the person's game exposes a
 * server; it opens Steam only after the user confirms.
 */
import { useEffect, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type NowGaming } from "../../api";
import { ConfirmDialog } from "../servers/ConfirmDialog";
import "./nowGaming.css";
import { GAMEPAD_PATH, playedAt, playedParts, type LiveGame } from "./playTime";
import { useNowGaming } from "./nowGamingStore";

/** How often the play time is refreshed (it shows whole minutes). */
export const PLAYED_TICK_MS = 30_000;

/** The block for `session`, or nothing while the person shares no game. */
export function NowGamingFor({
  session,
  compact = false,
  joinable = false,
}: {
  session: number;
  compact?: boolean;
  /** Offer "Join game" (not for the own game, not in hover previews). */
  joinable?: boolean;
}) {
  const live = useNowGaming(session);
  return live ? (
    <NowGamingBlock session={session} live={live} compact={compact} joinable={joinable} />
  ) : null;
}

export function NowGamingBlock({
  session,
  live,
  compact = false,
  joinable = false,
}: {
  session: number;
  live: LiveGame;
  compact?: boolean;
  joinable?: boolean;
}) {
  const { t } = useTranslation("members");
  const { game } = live;
  const icon = useIcon(session, game);
  const now = useClock();
  const { hours, minutes } = playedParts(playedAt(live, now));
  const time =
    hours > 0
      ? t("nowGaming.hoursMinutes", { hours, minutes })
      : t("nowGaming.minutes", { count: minutes });
  const className = compact ? "now-gaming now-gaming--compact" : "now-gaming";
  return (
    <section className={className} aria-label={t("nowGaming.label")}>
      {icon !== null ? (
        <img className="now-gaming__icon" src={icon} alt="" />
      ) : (
        <svg
          className="now-gaming__icon now-gaming__icon--generic"
          data-icon="gamepad"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
          focusable="false"
        >
          <path d={GAMEPAD_PATH} />
        </svg>
      )}
      <div className="now-gaming__text">
        <p className="now-gaming__state">{t("nowGaming.playing")}</p>
        <p className="now-gaming__name">{game.name}</p>
        <p className="now-gaming__time">{t("nowGaming.playedFor", { time })}</p>
      </div>
      {joinable && game.canJoin && <JoinButton session={session} name={game.name} />}
    </section>
  );
}

function JoinButton({ session, name }: { session: number; name: string }) {
  const { t } = useTranslation("members");
  const [confirming, setConfirming] = useState(false);
  const [failed, setFailed] = useState(false);
  const join = () => {
    setConfirming(false);
    getApi()
      .nowGamingJoin(session)
      .then(
        () => {
          setFailed(false);
        },
        (e: unknown) => {
          console.error("Joining the game failed", e);
          setFailed(true);
        },
      );
  };
  // Escape closes only the confirmation, not the profile card around it.
  const keepEscape = (event: KeyboardEvent) => {
    if (event.key === "Escape") event.stopPropagation();
  };
  return (
    <>
      <button
        type="button"
        className="now-gaming__join"
        onClick={() => {
          setConfirming(true);
        }}
      >
        {t("nowGaming.join")}
      </button>
      {failed && (
        <p className="now-gaming__error" role="alert">
          {t("nowGaming.joinFailed")}
        </p>
      )}
      {confirming && (
        // eslint-disable-next-line jsx-a11y/no-static-element-interactions -- only keeps Escape inside the confirmation
        <div onKeyDown={keepEscape}>
          <ConfirmDialog
            title={t("nowGaming.joinTitle")}
            message={t("nowGaming.joinMessage", { game: name })}
            confirmLabel={t("nowGaming.joinConfirm")}
            onConfirm={join}
            onClose={() => {
              setConfirming(false);
            }}
          />
        </div>
      )}
    </>
  );
}

/** Current time, refreshed every {@link PLAYED_TICK_MS}. */
function useClock(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => {
      setNow(Date.now());
    }, PLAYED_TICK_MS);
    return () => {
      clearInterval(timer);
    };
  }, []);
  return now;
}

/** The game icon as a `data:` URL from the engine; reloaded when the game changes. */
function useIcon(session: number, game: NowGaming): string | null {
  const { hasIcon, name } = game;
  const key = `${String(session)}\n${name}`;
  const [loaded, setLoaded] = useState<{ key: string; icon: string | null } | null>(null);
  useEffect(() => {
    if (!hasIcon) return;
    let active = true;
    getApi()
      .nowGamingIcon(session)
      .then(
        (icon) => {
          if (active) setLoaded({ key, icon });
        },
        (e: unknown) => {
          console.error("Loading a game icon failed", e);
        },
      );
    return () => {
      active = false;
    };
  }, [session, hasIcon, key]);
  return hasIcon && loaded?.key === key ? loaded.icon : null;
}
