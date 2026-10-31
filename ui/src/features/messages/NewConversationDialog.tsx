import { useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { User } from "../../api";
import { useServers as useConnectedServers } from "../channels";
import { openPrivateConversation } from "../members/privateMessage";
import { Avatar } from "../members/Avatar";
import { Dialog } from "../servers/Dialog";
import { displayName } from "../servers/format";
import { useServers as useSavedServers } from "../servers/store";

/** "Start a new conversation": pick anyone on any connected server, found by name. */
export function NewConversationDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation("chat");
  const connected = useConnectedServers();
  const { servers } = useSavedServers();
  const [query, setQuery] = useState("");
  const searchRef = useRef<HTMLInputElement>(null);
  const searchId = useId();
  const q = query.trim().toLocaleLowerCase();

  // In the order of the server bar; servers the saved list does not know yet go last.
  const order = (id: number) => {
    const index = servers.findIndex((s) => s.id === id);
    return index === -1 ? servers.length : index;
  };
  const groups = [...connected]
    .sort(([a], [b]) => order(a) - order(b))
    .map(([serverId, state]) => {
      const saved = servers.find((s) => s.id === serverId);
      const people: User[] = [...state.users.values()]
        .filter((u) => u.session !== state.selfSession)
        .filter((u) => q === "" || u.name.toLocaleLowerCase().includes(q))
        .sort((a, b) => a.name.localeCompare(b.name));
      return { serverId, name: saved ? displayName(saved) : "", people };
    })
    .filter((g) => g.people.length > 0);

  return (
    <Dialog title={t("overview.newTitle")} onClose={onClose} initialFocus={searchRef}>
      <label className="visually-hidden" htmlFor={searchId}>
        {t("overview.newSearch")}
      </label>
      <input
        ref={searchRef}
        id={searchId}
        type="search"
        className="pm-new__search"
        placeholder={t("overview.newSearch")}
        value={query}
        onChange={(event) => {
          setQuery(event.target.value);
        }}
      />
      {groups.length === 0 ? (
        <p className="pm-new__empty" role="status">
          {t("overview.newEmpty")}
        </p>
      ) : (
        <div className="pm-new__groups">
          {groups.map((group) => (
            <section key={group.serverId} aria-label={group.name || undefined}>
              {group.name !== "" && <h3 className="pm-section__title">{group.name}</h3>}
              <ul className="pm-new__list">
                {group.people.map((user) => (
                  <li key={user.session}>
                    <button
                      type="button"
                      className="pm-new__person"
                      data-testid="pm-new-person"
                      onClick={() => {
                        openPrivateConversation(user, group.serverId);
                        onClose();
                      }}
                    >
                      <Avatar name={user.name} size="sm" />
                      <span>{user.name}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          ))}
        </div>
      )}
    </Dialog>
  );
}
