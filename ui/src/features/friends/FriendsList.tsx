import { useTranslation } from "react-i18next";
import { getApi, type Conversation } from "../../api";
import {
  sameConversation,
  selectConversation,
  useSelectedConversation,
  useServerState,
  useServers as useConnectedServers,
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
  const connected = useConnectedServers();
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
            const status = presenceText(t, friend, i18n.language, {
              shownServerId: server.serverId,
              serverName: (id) => servers.find((s) => s.id === id)?.label,
              channelName: (id, channelId) => connected.get(id)?.channels.get(channelId)?.name,
            });
            // Sessions only mean something on the connected server.
            const session =
              friend.online !== null && friend.online.serverId === server.serverId
                ? friend.online.session
                : null;
            // A friend on another connected server: Join moves voice there (FR-2.12).
            const elsewhere =
              friend.online !== null && friend.online.serverId !== server.serverId
                ? friend.online
                : null;
            return (
              <li key={friend.certHash} className="friends-list__row">
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
                {elsewhere !== null && (
                  <button
                    type="button"
                    className="button friends-list__join"
                    aria-label={t("joinFriend", { name: friend.name })}
                    onClick={() => {
                      getApi()
                        .channelJoin(elsewhere.serverId, elsewhere.channelId)
                        .catch((error: unknown) => {
                          console.error("Joining a friend's channel failed", error);
                        });
                    }}
                  >
                    {t("join")}
                  </button>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </CollapsibleSection>
  );
}
