import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { toEngineError, type BlockedMessages } from "../../../api";
import { noteBlockedMessages, unblock, useBlockedUsers } from "../../blocking";
import "../../blocking/blocking.css";
import { Avatar } from "../../members/Avatar";
import { useServers } from "../../servers/store";
import { Group } from "../components/Section";
import type { SettingsSectionProps } from "./types";
import "./privateMessages.css";

/**
 * Everyone the user blocked (issue #140): name, the server it was set on and since when, each with
 * Unblock. Blocks of other identities are listed too; the identity tells them apart.
 */
export function BlockedUsersGroup() {
  const { t, i18n } = useTranslation("settings");
  const users = useBlockedUsers();
  const { identities } = useServers();
  const [failed, setFailed] = useState(false);
  const date = new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium" });
  const identityLabel = (certHash: string) =>
    identities.length > 1 ? identities.find((i) => i.certHash === certHash)?.label : undefined;
  return (
    <Group title={t("blocked.title")} icon="ban" info={t("blocked.hint")} wide>
      {users.length === 0 ? (
        <p className="settings-hint" data-testid="blocked-empty">
          {t("blocked.empty")}
        </p>
      ) : (
        <table className="blocked-table" data-testid="blocked-table">
          <thead>
            <tr>
              <th scope="col">{t("blocked.user")}</th>
              <th scope="col">{t("blocked.server")}</th>
              <th scope="col">{t("blocked.since")}</th>
              <th scope="col">
                <span className="visually-hidden">{t("blocked.actions")}</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {users.map((user) => {
              const identity = identityLabel(user.ownCertHash);
              return (
                <tr key={user.id}>
                  <td>
                    <span className="blocked-table__user">
                      <Avatar name={user.name} size="sm" />
                      {/* Untrusted plain text. */}
                      <span>
                        {user.name}
                        {identity !== undefined && (
                          <span className="settings-hint">
                            {" "}
                            · {t("acceptedConversations.identity", { label: identity })}
                          </span>
                        )}
                      </span>
                    </span>
                  </td>
                  <td>{user.serverLabel}</td>
                  <td className="blocked-table__since">{date.format(new Date(user.blockedAt))}</td>
                  <td className="blocked-table__action">
                    <button
                      type="button"
                      className="settings-button"
                      aria-label={t("blocked.unblockLabel", { name: user.name })}
                      onClick={() => {
                        setFailed(false);
                        unblock(user.id).catch((error: unknown) => {
                          console.error("Unblocking failed", toEngineError(error).code);
                          setFailed(true);
                        });
                      }}
                    >
                      {t("blocked.unblock")}
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
      {failed && (
        <p role="alert" className="settings-error">
          {t("blocked.unblockFailed")}
        </p>
      )}
    </Group>
  );
}

const MODES: readonly BlockedMessages[] = ["collapse", "hide"];

/** Collapse or hide channel messages of blocked users; radios drawn as cards. */
export function BlockedMessagesGroup({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const id = useId();
  return (
    <Group title={t("blocked.messagesTitle")} icon="eyeOff">
      <fieldset className="pm-rules">
        <legend className="visually-hidden">{t("blocked.messagesTitle")}</legend>
        {MODES.map((mode) => {
          const titleId = `${id}-${mode}-title`;
          const hintId = `${id}-${mode}-hint`;
          return (
            <label key={mode} className="pm-rules__card">
              <input
                type="radio"
                name={id}
                value={mode}
                checked={settings.blockedMessages === mode}
                aria-labelledby={titleId}
                aria-describedby={hintId}
                data-testid={`blocked-messages-${mode}`}
                onChange={() => {
                  update({ blockedMessages: mode });
                  noteBlockedMessages(mode);
                }}
              />
              <span className="pm-rules__text">
                <span id={titleId} className="pm-rules__title">
                  {t(`blocked.${mode}`)}
                </span>
                <span id={hintId} className="pm-rules__hint">
                  {t(`blocked.${mode}Hint`)}
                </span>
              </span>
            </label>
          );
        })}
      </fieldset>
    </Group>
  );
}
