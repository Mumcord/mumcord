import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toEngineError } from "../../../api";
import { revokeAccepted, useAcceptedConversations } from "../../requests";
import { useServers } from "../../servers/store";
import { Group } from "../components/Section";

/** People who may keep writing despite the private message rule; each can be revoked (issue #141). */
export function AcceptedConversationsGroup() {
  const { t, i18n } = useTranslation("settings");
  const conversations = useAcceptedConversations();
  const { identities } = useServers();
  const [failed, setFailed] = useState(false);
  const date = new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium" });
  // The identity only tells entries apart when there is more than one.
  const identityLabel = (certHash: string) =>
    identities.length > 1 ? identities.find((i) => i.certHash === certHash)?.label : undefined;
  return (
    <Group title={t("acceptedConversations.title")} icon="check">
      {conversations.length === 0 ? (
        <p className="settings-hint">{t("acceptedConversations.empty")}</p>
      ) : (
        <ul className="pm-accepted">
          {conversations.map((conversation) => {
            const label = identityLabel(conversation.ownCertHash);
            return (
              <li
                key={`${conversation.ownCertHash}:${conversation.certHash}`}
                className="pm-accepted__row"
              >
                <span className="pm-accepted__text">
                  <span className="pm-accepted__name">{conversation.name}</span>
                  <span className="settings-hint">
                    {t("acceptedConversations.acceptedAt", {
                      date: date.format(new Date(conversation.acceptedAt)),
                    })}
                    {label !== undefined && (
                      <> · {t("acceptedConversations.identity", { label })}</>
                    )}
                  </span>
                </span>
                <button
                  type="button"
                  className="settings-button"
                  aria-label={t("acceptedConversations.revokeLabel", { name: conversation.name })}
                  onClick={() => {
                    setFailed(false);
                    revokeAccepted(conversation).catch((error: unknown) => {
                      console.error(
                        "Revoking an accepted conversation failed",
                        toEngineError(error).code,
                      );
                      setFailed(true);
                    });
                  }}
                >
                  {t("acceptedConversations.revoke")}
                </button>
              </li>
            );
          })}
        </ul>
      )}
      {failed && (
        <p role="alert" className="settings-error">
          {t("acceptedConversations.revokeFailed")}
        </p>
      )}
    </Group>
  );
}
