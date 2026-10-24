/**
 * Member column: the people in the selected channel, then a "Listening" section. All people
 * share one Tab stop (roving tabindex, Up/Down/Home/End); Enter or click opens the profile card.
 */
import { useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import type { User } from "../../api";
import { StatusIcons } from "../channels/PersonStatus";
import { useStatusText } from "../channels/statusText";
import { useSelectedConversation } from "../channels/selection";
import { useServerState } from "../channels/serverStore";
import "./members.css";
import { Avatar } from "./Avatar";
import type { SendPrivateMessage } from "./privateMessage";
import { useAvatars } from "./useAvatars";
import { useProfileCard } from "./useProfileCard";

const byName = (a: User, b: User) =>
  a.name.localeCompare(b.name, undefined, { sensitivity: "base" });

export interface MembersPanelProps {
  /** "Send private message" on the profile card; defaults to opening the private chat. */
  onSendPrivateMessage?: SendPrivateMessage | undefined;
}

export function MembersPanel({ onSendPrivateMessage }: MembersPanelProps) {
  const { t } = useTranslation("members");
  const state = useServerState();
  const { conversation } = useSelectedConversation();
  const { openProfile, profileCard } = useProfileCard(onSendPrivateMessage);
  const statusText = useStatusText();
  const listRef = useRef<HTMLDivElement>(null);
  const [focusedKey, setFocusedKey] = useState<string | null>(null);

  const channel =
    conversation?.kind === "channel" ? state.channels.get(conversation.channelId) : undefined;
  const users = channel ? [...state.users.values()] : [];
  const people = users.filter((u) => u.channelId === channel?.id).sort(byName);
  const listeners = users
    .filter((u) => channel !== undefined && u.listeningChannelIds.includes(channel.id))
    .sort(byName);
  const avatarOf = useAvatars([...people, ...listeners]);

  if (!channel) {
    return <p className="members-empty">{t("noChannel")}</p>;
  }

  const keys = [
    ...people.map((u) => `p${String(u.session)}`),
    ...listeners.map((u) => `l${String(u.session)}`),
  ];
  const tabStop = keys.find((k) => k === focusedKey) ?? keys[0];

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const index = keys.findIndex((k) => k === tabStop);
    const targets: Record<string, number> = {
      ArrowDown: index + 1,
      ArrowUp: index - 1,
      Home: 0,
      End: keys.length - 1,
    };
    const targetIndex = targets[event.key];
    if (targetIndex === undefined) return;
    event.preventDefault();
    const target = keys[targetIndex];
    if (target === undefined) return;
    setFocusedKey(target);
    listRef.current?.querySelector<HTMLElement>(`[data-key="${target}"]`)?.focus();
  };

  const item = (user: User, key: string, listener: boolean) => {
    const talking = state.talking.has(user.session);
    const status = statusText(user, talking);
    return (
      <li key={key}>
        <button
          type="button"
          className="member"
          data-key={key}
          data-talking={talking}
          tabIndex={key === tabStop ? 0 : -1}
          onFocus={() => {
            setFocusedKey(key);
          }}
          onClick={(event) => {
            openProfile(user.session, event.currentTarget);
          }}
        >
          <Avatar name={user.name} src={avatarOf(user)} talking={talking} />
          <span className="member__name">
            {user.name}
            {user.session === state.selfSession && <span className="member__you"> {t("you")}</span>}
            {status.length > 0 && <span className="visually-hidden">, {status.join(", ")}</span>}
          </span>
          <StatusIcons user={user} listener={listener} />
        </button>
      </li>
    );
  };

  return (
    // The key handler moves focus between the buttons inside; it is not itself interactive.
    // eslint-disable-next-line jsx-a11y/no-static-element-interactions
    <div className="members-panel" ref={listRef} onKeyDown={onKeyDown}>
      <h2 className="members-heading">
        {t("inChannel", { channel: channel.name, count: people.length })}
      </h2>
      {people.length > 0 ? (
        <ul className="members-list">
          {people.map((u) => item(u, `p${String(u.session)}`, false))}
        </ul>
      ) : (
        <p className="members-none">{t("nobody")}</p>
      )}
      {listeners.length > 0 && (
        <>
          <h2 className="members-heading">{t("listening", { count: listeners.length })}</h2>
          <ul className="members-list">
            {listeners.map((u) => item(u, `l${String(u.session)}`, true))}
          </ul>
        </>
      )}
      {profileCard}
    </div>
  );
}
