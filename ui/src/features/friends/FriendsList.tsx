import { useTranslation } from "react-i18next";
import type { Conversation } from "../../api";
import {
  sameConversation,
  selectConversation,
  useSelectedConversation,
  useServerState,
} from "../channels";
import {
  CollapsibleSection,
  openPersonMenuFromEvent,
  openPersonMenuFromKey,
  useSectionOpen,
} from "../members";
import { useServers } from "../servers";
import { useFriends } from "./friendsStore";
import { presenceText } from "./presence";
import "./friends.css";

/**
 * Collapsible "Friends" section in the member column (FR-1.64): online first; a click opens the
 * private chat, a right-click the person menu.
 */
export function FriendsList() {
  const { t, i18n } = useTranslation("friends");
  const friends = useFriends();
  const server = useServerState();
  const { servers } = useServers();
  const { conversation } = useSelectedConversation();
  const [open, setOpen] = useSectionOpen("friends", true);

  if (friends.length === 0) return null;
  return (
    <CollapsibleSection
      id="friends"
      className="friends-list"
      title={t("title")}
      open={open}
      onOpenChange={setOpen}
    >
      {() => (
        <ul>
          {friends.map((friend) => {
            const chat: Conversation = { kind: "private", certHash: friend.certHash };
            const status = presenceText(
              t,
              friend,
              server,
              i18n.language,
              (id) => servers.find((s) => s.id === id)?.label,
            );
            // Sessions only mean something on the connected server.
            const session =
              friend.online !== null && friend.online.serverId === server.serverId
                ? friend.online.session
                : null;
            return (
              <li key={friend.certHash}>
                <button
                  type="button"
                  className="friends-list__item"
                  aria-current={sameConversation(conversation, chat)}
                  aria-label={t("item", { name: friend.name, status })}
                  onContextMenu={(event) => {
                    openPersonMenuFromEvent(session, event, friend.name);
                  }}
                  onKeyDown={(event) => {
                    openPersonMenuFromKey(session, event, friend.name);
                  }}
                  onClick={() => {
                    selectConversation(chat);
                  }}
                >
                  <span
                    className="friends-list__dot"
                    data-online={friend.online !== null}
                    aria-hidden="true"
                  />
                  <span className="friends-list__text">
                    <span className="friends-list__name">{friend.name}</span>
                    <span className="friends-list__status">{status}</span>
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </CollapsibleSection>
  );
}
