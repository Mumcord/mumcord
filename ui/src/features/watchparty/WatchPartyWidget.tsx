/**
 * The watch party of the voice server in a floating widget over the app (FR-2.40, FR-2.70),
 * mounted at app-shell level so it stays open while switching chats. It renders only while the user has joined (FR-2.40); an unjoined party shows just the activity
 * banner. Collapsed it is a slim bar
 * with the title and play/pause; "Leave party" inside the panel closes it.
 */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { getApi, type User } from "../../api";
import { FloatingWidget } from "../../components/FloatingWidget";
import type { Rect, Size } from "../../components/floatingWidgetGeometry";
import { useServerStateOf, useVoiceServerId } from "../channels";
import { Avatar } from "../members/Avatar";
import { useAvatars } from "../members/useAvatars";
import { WatchLinkDialog } from "./WatchLinkDialog";
import { PlayPause } from "./PlayPause";
import { WatchPartyPanel } from "./WatchPartyPanel";
import { useWatchParty } from "./watchPartyStore";
import "./watchPartyWidget.css";

const VIDEO_ASPECT = 16 / 9;
/** Control row height and its gap above; must match `.watch-playlist__header` in watchPartyWidget.css. */
export const CONTROL_ROW = 28;
export const CONTROL_GAP = 4;
/** The widget's 1px border top and bottom; the title bar measure does not count it. */
export const WIDGET_BORDER = 2;
/** Height below the video: the control row, its gap and the frame; notices overlay the video. */
export const EXTRA = CONTROL_ROW + CONTROL_GAP + WIDGET_BORDER;

/** Opens top right like a screen share, a little lower so both stay visible. */
function initialRect(window: Size): Rect {
  const width = 420;
  const height = width / VIDEO_ASPECT + 40 + EXTRA;
  return { x: window.width - width - 312, y: 112, width, height };
}

export function WatchPartyWidget() {
  const { t } = useTranslation("watchparty");
  const serverId = useVoiceServerId();
  const live = useWatchParty(serverId);
  const [adding, setAdding] = useState(false);
  if (serverId === null || live === null || !live.party.joined) return null;
  const { party } = live;
  const title = (
    <>
      {t("title")} <span className="watch-party__beta">{t("beta")}</span>
    </>
  );
  return (
    <FloatingWidget
      kind="watchParty"
      className="watch-party-widget"
      label={t("title")}
      title={title}
      titleExtra={<Participants serverId={serverId} sessions={party.participants} />}
      actions={
        <>
          <LeaveParty serverId={serverId} />
          <button
            type="button"
            className="button"
            data-tooltip={t("addVideo")}
            aria-label={t("addVideo")}
            onClick={() => {
              setAdding(true);
            }}
          >
            +
          </button>
        </>
      }
      collapsedActions={<PlayPause serverId={serverId} live={live} />}
      aspect={VIDEO_ASPECT}
      extra={EXTRA}
      initialRect={initialRect}
    >
      <WatchPartyPanel serverId={serverId} channelId={live.party.channelId} />
      {adding && (
        <WatchLinkDialog
          serverId={serverId}
          onClose={() => {
            setAdding(false);
          }}
        />
      )}
    </FloatingWidget>
  );
}

/** At most this many avatars show; the rest collapses into "+N". */
const MAX_AVATARS = 3;

function Participants({ serverId, sessions }: { serverId: number; sessions: readonly number[] }) {
  const { t } = useTranslation("watchparty");
  const { users } = useServerStateOf(serverId);
  const people = sessions.flatMap((session) => {
    const user = users.get(session);
    return user ? [user] : [];
  });
  const avatarOf = useAvatars(people, serverId);
  if (people.length === 0) return null;
  const shown = people.slice(0, MAX_AVATARS);
  const more = people.length - shown.length;
  return (
    <div
      className="watch-party-widget__people"
      role="img"
      aria-label={t("watching", {
        count: people.length,
        names: people.map((user: User) => user.name).join(", "),
      })}
    >
      {shown.map((user) => (
        <span key={user.session} className="watch-party-widget__person" data-tooltip={user.name}>
          <Avatar name={user.name} src={avatarOf(user)} size="sm" />
        </span>
      ))}
      {more > 0 && (
        <span
          className="watch-party-widget__more"
          data-tooltip={people
            .slice(MAX_AVATARS)
            .map((user) => user.name)
            .join(", ")}
        >
          +{more}
        </span>
      )}
    </div>
  );
}

function LeaveParty({ serverId }: { serverId: number }) {
  const { t } = useTranslation("watchparty");
  const onClick = () => {
    getApi()
      .watchPartyLeave(serverId)
      .catch((error: unknown) => {
        console.error(t("leaveFailed"), error);
        announce(t("leaveFailed"), "assertive");
      });
  };
  return (
    <button
      type="button"
      className="button"
      data-tooltip={t("leave")}
      aria-label={t("leave")}
      onClick={onClick}
    >
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" focusable="false">
        <path
          d="M6 2H3v12h3M10 5l3 3-3 3M13 8H6"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.6"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </svg>
    </button>
  );
}
