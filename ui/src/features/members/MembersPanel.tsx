/**
 * Member column: the people in the selected channel, then a "Listening" section. All people
 * share one Tab stop (roving tabindex, Up/Down/Home/End); Enter or click opens the profile card,
 * hover or keyboard focus shows the profile preview.
 */
import { useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import type { User } from "../../api";
import { StatusIcons } from "../channels/PersonStatus";
import { MumcordBadge } from "../channels/MumcordBadge";
import { useStatusText } from "../channels/statusText";
import { useSelectedConversation } from "../channels/selection";
import { useServerState } from "../channels/serverStore";
import { buildTree, type ChannelNode } from "../channels/tree";
import "./members.css";
import { CollapsibleSection } from "./CollapsibleSection";
import { useSectionOpen } from "./useSectionOpen";
import { Avatar } from "./Avatar";
import type { SendPrivateMessage } from "./privateMessage";
import { useAvatars } from "./useAvatars";
import { ProfilePreviewTrigger } from "./ProfilePreview";
import { clickPerson, doubleClickPerson } from "./personClick";

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
  const statusText = useStatusText();
  const listRef = useRef<HTMLDivElement>(null);
  const [focusedKey, setFocusedKey] = useState<string | null>(null);

  const channel =
    conversation?.kind === "channel" ? state.channels.get(conversation.channelId) : undefined;
  const users = channel ? [...state.users.values()] : [];
  const people = users.filter((u) => u.channelId === channel?.id).sort(byName);
  const listeners = users
    .filter(
      (u) =>
        channel !== undefined &&
        u.channelId !== channel.id &&
        u.listeningChannelIds.includes(channel.id),
    )
    .sort(byName);
  const [peopleOpen, setPeopleOpen] = useSectionOpen("members-here", true);
  const [othersOpen, setOthersOpen] = useSectionOpen("members-others", false);
  // Walking the whole server is only worth it while the section is open.
  const others = channel && othersOpen ? otherGroups(buildTree(state), channel.id) : [];
  const avatarOf = useAvatars([
    ...(peopleOpen ? people : []),
    ...listeners,
    ...others.flatMap((g) => g.members),
  ]);

  if (!channel) {
    return <p className="members-empty">{t("noChannel")}</p>;
  }

  const keys = [
    ...(peopleOpen ? people : []).map((u) => `p${String(u.session)}`),
    ...listeners.map((u) => `l${String(u.session)}`),
    ...others.flatMap((g) => g.members.map((u) => `o${String(u.session)}`)),
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
    const talking = !listener && state.talking.has(user.session);
    const status = statusText(user, talking, listener);
    return (
      <li key={key}>
        <ProfilePreviewTrigger session={user.session}>
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
              clickPerson(event.detail, user.session, event.currentTarget);
            }}
            onDoubleClick={() => {
              doubleClickPerson(user.session, undefined, onSendPrivateMessage);
            }}
          >
            <Avatar name={user.name} src={avatarOf(user)} talking={talking} />
            <span className="member__name" data-local-muted={user.localMute}>
              {user.name}
              <MumcordBadge user={user} />
              {user.session === state.selfSession && (
                <span className="member__you"> {t("you")}</span>
              )}
              {status.length > 0 && <span className="visually-hidden">, {status.join(", ")}</span>}
            </span>
            <StatusIcons user={user} listener={listener} />
          </button>
        </ProfilePreviewTrigger>
      </li>
    );
  };

  return (
    // The key handler moves focus between the buttons inside; it is not itself interactive.
    // eslint-disable-next-line jsx-a11y/no-static-element-interactions
    <div className="members-panel" ref={listRef} onKeyDown={onKeyDown}>
      <CollapsibleSection
        id="members-here"
        title={t("inChannel", { channel: channel.name, count: people.length })}
        open={peopleOpen}
        onOpenChange={setPeopleOpen}
      >
        {() =>
          people.length > 0 ? (
            <ul className="members-list">
              {people.map((u) => item(u, `p${String(u.session)}`, false))}
            </ul>
          ) : (
            <p className="members-none">{t("nobody")}</p>
          )
        }
      </CollapsibleSection>
      {listeners.length > 0 && (
        <>
          <h2 className="members-heading">{t("listening", { count: listeners.length })}</h2>
          <ul className="members-list">
            {listeners.map((u) => item(u, `l${String(u.session)}`, true))}
          </ul>
        </>
      )}
      <CollapsibleSection
        id="members-others"
        title={t("inOtherChannels")}
        open={othersOpen}
        onOpenChange={setOthersOpen}
      >
        {() =>
          others.length > 0 ? (
            others.map((group) => (
              <div key={group.channel.id}>
                <h3 className="members-heading">{group.channel.name}</h3>
                <ul className="members-list">
                  {group.members.map((u) => item(u, `o${String(u.session)}`, false))}
                </ul>
              </div>
            ))
          ) : (
            <p className="members-none">{t("nobodyElse")}</p>
          )
        }
      </CollapsibleSection>
    </div>
  );
}

interface ChannelGroup {
  channel: ChannelNode["channel"];
  members: User[];
}

/** People outside `currentId`, grouped by channel in channel-tree order (empty channels skipped). */
function otherGroups(roots: readonly ChannelNode[], currentId: number): ChannelGroup[] {
  const groups: ChannelGroup[] = [];
  const walk = (node: ChannelNode) => {
    if (node.channel.id !== currentId && node.members.length > 0) {
      groups.push({ channel: node.channel, members: node.members });
    }
    node.children.forEach(walk);
  };
  roots.forEach(walk);
  return groups;
}
