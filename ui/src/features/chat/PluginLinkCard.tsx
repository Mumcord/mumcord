/**
 * Card under a message for a `mumcord://plugins/<id>` link (FR-6.65a): the plugin's icon, name,
 * summary and authors, with Install or Open, and Details. Only the id comes from the untrusted
 * message; every text shown is the plugin index's and React renders it as text. Ids that are not
 * official plugins get no card, and Install only opens the install prompt.
 */
import { useTranslation } from "react-i18next";
import { PluginIcon } from "../plugins/browse/PluginIcon";
import { useSettledPluginIndex } from "../plugins/browse/indexStore";
import { routePluginRequest } from "../plugins/pluginRequestStore";
import { usePlugins } from "../plugins/pluginStore";
import { DismissButton } from "./cardButtons";

interface PluginLinkCardProps {
  /** Plugin id from the link, already checked by `parsePluginLink`. */
  id: string;
  tabIndex: number;
  onDismiss?: () => void;
}

export function PluginLinkCard({ id, tabIndex, onDismiss }: PluginLinkCardProps) {
  const { t, i18n } = useTranslation("chat");
  const { t: tBrowse } = useTranslation("pluginBrowse");
  const index = useSettledPluginIndex(i18n.language);
  const { overview, loadFailed } = usePlugins();

  if (index.status === "failed") return null;
  // Install or Open is only known once the installed plugins are; until then nothing is offered.
  if (index.status === "loading" || (overview === null && !loadFailed)) {
    return (
      <article
        className="message-card plugin-link-card plugin-link-card--loading"
        aria-busy="true"
        aria-label={t("pluginCard.loading")}
      >
        <span className="plugin-link-card__skeleton plugin-link-card__skeleton--icon" />
        <div className="plugin-link-card__body">
          <span className="plugin-link-card__skeleton plugin-link-card__skeleton--name" />
          <span className="plugin-link-card__skeleton plugin-link-card__skeleton--summary" />
          <span className="plugin-link-card__skeleton plugin-link-card__skeleton--authors" />
          <div className="plugin-link-card__actions">
            <span className="plugin-link-card__skeleton plugin-link-card__skeleton--button" />
            <span className="plugin-link-card__skeleton plugin-link-card__skeleton--button" />
          </div>
        </div>
      </article>
    );
  }

  const plugin = index.index.plugins.find(
    (entry) => entry.id === id && entry.source.kind === "official",
  );
  if (plugin === undefined) return null;
  const name = plugin.name;
  const installed = overview?.plugins.some((entry) => entry.id === id) ?? false;

  return (
    <article
      className="message-card plugin-link-card"
      data-plugin-id={id}
      aria-label={t("pluginCard.label", { name })}
    >
      <PluginIcon id={id} name={name} hasIcon={plugin.icon !== null} />
      <div className="plugin-link-card__body">
        <div className="plugin-link-card__top">
          <p className="plugin-link-card__name">{name}</p>
          <span className="plugin-link-card__tag">{tBrowse("official")}</span>
          {installed && (
            <span className="plugin-link-card__installed">✓ {tBrowse("installed")}</span>
          )}
        </div>
        <p className="plugin-link-card__summary">{plugin.summary}</p>
        {plugin.authors.length > 0 && (
          <p className="plugin-link-card__authors">
            {tBrowse("by", { authors: plugin.authors.join(", ") })}
          </p>
        )}
        <div className="plugin-link-card__actions">
          {installed ? (
            <button
              type="button"
              className="plugin-link-card__button plugin-link-card__button--primary"
              tabIndex={tabIndex}
              aria-label={t("pluginCard.openLabel", { name })}
              onClick={() => {
                routePluginRequest({ kind: "installed", plugin: id });
              }}
            >
              {t("card.open")}
            </button>
          ) : (
            <button
              type="button"
              className="plugin-link-card__button plugin-link-card__button--primary"
              tabIndex={tabIndex}
              aria-label={tBrowse("installNamed", { name })}
              onClick={() => {
                // Opens the install prompt; the user confirms there, nothing installs from here.
                routePluginRequest({ kind: "install", plugin: id });
              }}
            >
              {tBrowse("install")}
            </button>
          )}
          <button
            type="button"
            className="plugin-link-card__button"
            tabIndex={tabIndex}
            aria-label={tBrowse("detailsNamed", { name })}
            onClick={() => {
              routePluginRequest({ kind: "details", plugin: id });
            }}
          >
            {tBrowse("details")}
          </button>
        </div>
      </div>
      {onDismiss !== undefined && <DismissButton onDismiss={onDismiss} tabIndex={tabIndex} />}
    </article>
  );
}
