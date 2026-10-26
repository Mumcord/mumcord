/**
 * Member column: the people in the selected channel (listeners inline, marked with an ear). All people
 * share one Tab stop (roving tabindex, Up/Down/Home/End); Enter or click opens the profile card,
 * hover or keyboard focus shows the profile preview.
 */
import {
  memo,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type KeyboardEvent,
} from "react";
import { useTranslation } from "react-i18next";
import { useT } from "../../i18n/useT";
import type { User } from "../../api";
import { StatusIcons } from "../channels/PersonStatus";
import { MumcordBadge } from "../channels/MumcordBadge";
import { LiveAvatar, LiveBadge } from "../screenshare/LiveBadge";
import { useStatusText } from "../channels/statusText";
import { useSelectedConversation } from "../channels/selection";
import { useServerState, type ServerState } from "../channels/serverStore";
import { useListPreferences } from "../../app/preferences";
import { BigServerGate } from "../channels/BigServerGate";
import { useDrawSteps } from "../channels/drawSteps";
import { onScreenTracker, type OnScreen } from "../channels/treeContext";
import {
  BIG_SERVER_CHANNELS,
  buildTree,
  talkingPeopleFirst,
  type ChannelNode,
} from "../channels/tree";
import "./members.css";
import { CollapsibleSection } from "./CollapsibleSection";
import { useSectionOpen } from "./useSectionOpen";
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

/** People a crowded member list adds per frame on a big server. */
const MEMBER_STEP = 40;

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
  const members = users.filter((u) => u.channelId === channel?.id);
  const listeners = users.filter(
    (u) =>
      channel !== undefined &&
      u.channelId !== channel.id &&
      u.listeningChannelIds.includes(channel.id),
  );
  const { talkingFirst } = useListPreferences();
  const talks = (user: User) => talkingFirst && state.talking.has(user.session);
  const people = talkingPeopleFirst(
    [
      ...members.map((user) => ({ user, listener: false })),
      ...listeners.map((user) => ({ user, listener: true })),
    ].sort((a, b) => byName(a.user, b.user)),
    // Listeners hear the channel but do not talk in it.
    (p) => !p.listener && talks(p.user),
  );
  // A crowded channel on a big server is listed a step per frame (FR-2.24).
  const drawn = useDrawSteps(
    people.length,
    MEMBER_STEP,
    state.channels.size > BIG_SERVER_CHANNELS,
    `${String(state.serverId)}:${String(channel?.id)}`,
  );
  const shownPeople = people.slice(0, drawn);
  const [peopleOpen, setPeopleOpen] = useSectionOpen("members-here", true);
  const [othersOpen, setOthersOpen] = useSectionOpen("members-others", false);
  // Walking the whole server is only worth it while the section is open.
  const groups = useOtherGroups(
    state.channels,
    state.users,
    othersOpen ? (channel?.id ?? null) : null,
  );
  const allOthers = groups.map((group) => ({
    ...group,
    members: talkingPeopleFirst(group.members, talks),
  }));
  // Everyone else on a big server can be a thousand people: listed a step per frame as well, or
  // showing the server would draw them all at once and freeze the window (AC-2.17).
  const othersDrawn = useDrawSteps(
    allOthers.reduce((sum, group) => sum + group.members.length, 0),
    MEMBER_STEP,
    state.channels.size > BIG_SERVER_CHANNELS,
    `${String(state.serverId)}:${String(channel?.id)}:${String(othersOpen)}`,
  );
  const others = firstPeople(allOthers, othersDrawn);
  // Avatars load only for people whose rows came near the screen, as in the tree (scale report,
  // P5): a big server's list would otherwise request hundreds each time the server is shown.
  // Sessions are per server: another server starts with nobody seen.
  const [tracker, setTracker] = useState(() => ({
    serverId: state.serverId,
    onScreen: onScreenTracker(),
  }));
  let onScreen = tracker.onScreen;
  if (tracker.serverId !== state.serverId) {
    onScreen = onScreenTracker();
    setTracker({ serverId: state.serverId, onScreen });
  }
  const seen = useSyncExternalStore(onScreen.subscribe, onScreen.get);
  const avatarOf = useAvatars(
    [
      ...(peopleOpen ? shownPeople : []).map((p) => p.user),
      ...others.flatMap((g) => g.members),
    ].filter((user) => seen.has(user.session)),
  );

  if (!channel) {
    return <p className="members-empty">{t("noChannel")}</p>;
  }

  const keys = [
    ...(peopleOpen ? shownPeople : []).map(
      (p) => `${p.listener ? "l" : "p"}${String(p.user.session)}`,
    ),
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
    return (
      <MemberRow
        key={key}
        rowKey={key}
        user={user}
        listener={listener}
        talking={talking}
        tabbable={key === tabStop}
        avatar={avatarOf(user)}
        self={user.session === state.selfSession}
        status={statusText(user, talking, listener).join(", ")}
        onFocusRow={setFocusedKey}
        onSendPrivateMessage={onSendPrivateMessage}
        onScreen={onScreen}
      />
    );
  };

  return (
    <BigServerGate
      key={state.serverId}
      big={state.channels.size > BIG_SERVER_CHANNELS}
      className="members-empty"
    >
      {/* The key handler moves focus between the buttons inside; it is not itself interactive. */}
      {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions */}
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
                {shownPeople.map(({ user, listener }) =>
                  item(user, `${listener ? "l" : "p"}${String(user.session)}`, listener),
                )}
              </ul>
            ) : (
              <p className="members-none">{t("nobody")}</p>
            )
          }
        </CollapsibleSection>
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
    </BigServerGate>
  );
}

interface MemberRowProps {
  rowKey: string;
  user: User;
  listener: boolean;
  talking: boolean;
  tabbable: boolean;
  avatar: string | null;
  self: boolean;
  /** Screen-reader status, comma separated; empty for none. */
  status: string;
  onFocusRow: (key: string) => void;
  onSendPrivateMessage: SendPrivateMessage | undefined;
  onScreen: OnScreen;
}

/**
 * One person. Memoised: a big server's list grows a step per frame, and re-rendering every row
 * already drawn on each step made the steps slower and slower.
 */
const MemberRow = memo(function MemberRow({
  rowKey,
  user,
  listener,
  talking,
  tabbable,
  avatar,
  self,
  status,
  onFocusRow,
  onSendPrivateMessage,
  onScreen,
}: MemberRowProps) {
  const t = useT("members");
  const ref = useRef<HTMLLIElement>(null);
  useEffect(() => {
    const element = ref.current;
    return element === null ? undefined : onScreen.watch(element, [user.session]);
  }, [onScreen, user.session]);
  return (
    <li ref={ref}>
      <ProfilePreviewTrigger session={user.session}>
        <button
          type="button"
          className="member"
          data-key={rowKey}
          data-talking={talking}
          tabIndex={tabbable ? 0 : -1}
          onFocus={() => {
            onFocusRow(rowKey);
          }}
          onClick={(event) => {
            clickPerson(event.detail, user.session, event.currentTarget);
          }}
          onDoubleClick={() => {
            doubleClickPerson(user.session, undefined, onSendPrivateMessage);
          }}
        >
          <LiveAvatar session={user.session} name={user.name} src={avatar} talking={talking} />
          <span className="member__name" data-local-muted={user.localMute}>
            {user.name}
            <MumcordBadge user={user} />
            <LiveBadge session={user.session} />
            {self && <span className="member__you"> {t("you")}</span>}
            {status !== "" && <span className="visually-hidden">, {status}</span>}
          </span>
          <StatusIcons user={user} listener={listener} />
        </button>
      </ProfilePreviewTrigger>
    </li>
  );
});

interface ChannelGroup {
  channel: ChannelNode["channel"];
  members: User[];
}

/**
 * People outside `currentId` by channel (none for `null`), worked out again only when people or
 * channels change: not on every drawing step or talk change.
 */
function useOtherGroups(
  channels: ServerState["channels"],
  users: ServerState["users"],
  currentId: number | null,
): ChannelGroup[] {
  return useMemo(
    () => (currentId === null ? [] : otherGroups(buildTree({ channels, users }), currentId)),
    [channels, users, currentId],
  );
}

/** The groups holding the first `count` people, the last one cut short if needed. */
function firstPeople(groups: ChannelGroup[], count: number): ChannelGroup[] {
  const shown: ChannelGroup[] = [];
  let left = count;
  for (const group of groups) {
    if (left <= 0) break;
    shown.push(
      left >= group.members.length ? group : { ...group, members: group.members.slice(0, left) },
    );
    left -= group.members.length;
  }
  return shown;
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
