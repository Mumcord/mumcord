import { useTranslation } from "react-i18next";
import type { Conversation } from "../../api";
import {
  sameConversation,
  selectConversation,
  useSelectedConversation,
  useServerState,
} from "../channels";
import { useFriends } from "./friendsStore";
import { presenceText } from "./presence";
import "./friends.css";

/** "Friends" list below the channel tree (FR-1.64): online first; a click opens the private chat. */
export function FriendsList() {
  const { t, i18n } = useTranslation("friends");
  const friends = useFriends();
  const server = useServerState();
  const { conversation } = useSelectedConversation();

  if (friends.length === 0) return null;
  return (
    <section className="friends-list" aria-labelledby="friends-title">
      <h2 id="friends-title">{t("title")}</h2>
      <ul>
        {friends.map((friend) => {
          const chat: Conversation = { kind: "private", certHash: friend.certHash };
          const status = presenceText(t, friend, server, i18n.language);
          return (
            <li key={friend.certHash}>
              <button
                type="button"
                className="friends-list__item"
                aria-current={sameConversation(conversation, chat)}
                aria-label={t("item", { name: friend.name, status })}
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
    </section>
  );
}
