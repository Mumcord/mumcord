import { useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../../api";
import type { PluginItem } from "../../notifications/centreModel";
import { dismissItem, markRead, removeItems, setCentreOpen } from "../../notifications/centreStore";
import { openPluginItem } from "./pluginEntryText";
import { requestPluginScreen } from "./pluginRequests";
import type { PluginNotice } from "./types";

type Suggestion = Extract<PluginNotice, { kind: "suggestion" }>;

function SuggestionActions({ item, notice }: { item: PluginItem; notice: Suggestion }) {
  const { t } = useTranslation("pluginNotifications");
  const [failed, setFailed] = useState(false);
  const { name, plugin, serverId, source } = notice;

  const answer = (action: () => Promise<unknown>, done: () => void) => {
    setFailed(false);
    action().then(done, (error: unknown) => {
      console.error("Answering a plugin suggestion failed", error);
      setFailed(true);
    });
  };

  return (
    <>
      {source.kind === "official" ? (
        <>
          <button
            type="button"
            className="button button--primary centre-entry__action"
            aria-label={t("suggestion.installLabel", { name })}
            data-testid="centre-plugin-install"
            onClick={() => {
              markRead(item.id);
              setCentreOpen(false);
              requestPluginScreen({ kind: "install", plugin });
            }}
          >
            {t("suggestion.install")}
          </button>
          <button
            type="button"
            className="button centre-entry__action"
            aria-label={t("suggestion.detailsLabel", { name })}
            data-testid="centre-plugin-details"
            onClick={() => {
              markRead(item.id);
              setCentreOpen(false);
              requestPluginScreen({ kind: "details", plugin });
            }}
          >
            {t("suggestion.details")}
          </button>
        </>
      ) : (
        // A third-party plugin cannot be installed before its repository is known (FR-6.78), so
        // "Install…" leads to the Add repository dialog.
        <button
          type="button"
          className="button button--primary centre-entry__action"
          aria-label={t("suggestion.addRepositoryLabel", { name })}
          data-testid="centre-plugin-add-repository"
          onClick={() => {
            markRead(item.id);
            setCentreOpen(false);
            requestPluginScreen({ kind: "addRepository", url: source.url, key: source.key });
          }}
        >
          {t("suggestion.install")}
        </button>
      )}
      <button
        type="button"
        className="button centre-entry__action"
        aria-label={t("suggestion.notNowLabel", { name })}
        data-testid="centre-plugin-not-now"
        onClick={() => {
          answer(
            () => getApi().pluginSuggestionNotNow(serverId, plugin),
            () => {
              dismissItem(item.id);
            },
          );
        }}
      >
        {t("suggestion.notNow")}
      </button>
      <button
        type="button"
        className="centre-entry__link"
        data-testid="centre-plugin-dont-ask"
        onClick={() => {
          answer(
            () => getApi().pluginSuggestionsDismissServer(serverId),
            () => {
              removeItems(
                (other) =>
                  other.kind === "plugin" &&
                  other.notice.kind === "suggestion" &&
                  other.notice.serverId === serverId,
              );
            },
          );
        }}
      >
        {t("suggestion.dontAsk")}
      </button>
      {failed && (
        <span className="centre-entry__error" role="alert">
          {t("failed")}
        </span>
      )}
    </>
  );
}

/** The buttons under a plugin entry; entries without an answer to give have none. */
export function PluginActions({ item }: { item: PluginItem }) {
  const { t } = useTranslation("pluginNotifications");
  const { notice } = item;
  switch (notice.kind) {
    case "suggestion":
      return (
        <div className="centre-entry__actions">
          <SuggestionActions item={item} notice={notice} />
        </div>
      );
    case "update":
      return (
        <div className="centre-entry__actions">
          <button
            type="button"
            className="button centre-entry__action"
            data-testid="centre-plugin-updates"
            onClick={() => {
              openPluginItem(item);
            }}
          >
            {t("update.open")}
          </button>
        </div>
      );
    case "autoOff":
      return (
        <div className="centre-entry__actions">
          <button
            type="button"
            className="button centre-entry__action"
            data-testid="centre-plugin-open"
            onClick={() => {
              openPluginItem(item);
            }}
          >
            {t("autoOff.open")}
          </button>
        </div>
      );
    case "message":
      return null;
  }
}

/** Under the list when it holds suggestions: they never act on their own (FR-6.79). */
export function PluginSuggestionsNote() {
  const { t } = useTranslation("pluginNotifications");
  return <p className="centre__note">{t("suggestion.footer")}</p>;
}
