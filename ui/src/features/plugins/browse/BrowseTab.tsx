import { useEffect, useId, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { IndexedPlugin } from "../../../api";
import { consumePluginRequest, usePendingPluginRequest } from "../pluginRequestStore";
import { usePlugins } from "../pluginStore";
import { pluginMessageText } from "../prompt/errorText";
import { InstallPrompt } from "../prompt/InstallPrompt";
import "./browse.css";
import { loadPluginIndex, usePluginIndex } from "./indexStore";
import { PluginCard } from "./PluginCard";
import { PluginDetails } from "./PluginDetails";
import { CATEGORIES, visiblePlugins, type Category } from "./filter";
import { needsRepositoryWarning } from "./source";

/**
 * Settings → Plugins → Browse (FR-6.59): the official list (and enabled third-party repositories)
 * by category with search, a details view and the install prompt. Also answers "details" and
 * "install" requests from the notification centre and `mumcord://plugins/<id>` links.
 */
export function BrowseTab() {
  const { t, i18n } = useTranslation("pluginBrowse");
  // The plugin list texts follow the UI language.
  const language = i18n.language;
  const state = usePluginIndex(language);
  const { overview } = usePlugins();
  const installed = useMemo(() => overview?.plugins ?? [], [overview]);
  const [search, setSearch] = useState("");
  const [category, setCategory] = useState<Category>("all");
  const [detailsId, setDetailsId] = useState<string | null>(null);
  const [promptId, setPromptId] = useState<string | null>(null);
  const searchId = useId();
  const plugins = useMemo(() => (state.status === "loaded" ? state.index.plugins : []), [state]);
  const find = (id: string | null) => plugins.find((plugin) => plugin.id === id) ?? null;
  const isInstalled = (id: string) => installed.some((plugin) => plugin.id === id);

  const detailsRequest = usePendingPluginRequest("details");
  const installRequest = usePendingPluginRequest("install");
  const ready = state.status !== "loading";
  // Requests only open official plugins; an unknown id does nothing (FR-6.65). They are applied
  // while rendering (once each) and removed from the store afterwards.
  const [handled, setHandled] = useState<readonly object[]>([]);
  const official = (id: string) =>
    plugins.some((plugin) => plugin.id === id && plugin.source.kind === "official");
  if (ready && detailsRequest !== null && !handled.includes(detailsRequest)) {
    setHandled([detailsRequest]);
    if (official(detailsRequest.plugin)) setDetailsId(detailsRequest.plugin);
  }
  if (ready && installRequest !== null && !handled.includes(installRequest)) {
    setHandled([installRequest]);
    if (official(installRequest.plugin)) {
      setDetailsId(installRequest.plugin);
      if (!isInstalled(installRequest.plugin)) setPromptId(installRequest.plugin);
    }
  }
  useEffect(() => {
    if (!ready) return;
    if (detailsRequest !== null) consumePluginRequest("details");
    if (installRequest !== null) consumePluginRequest("install");
  }, [ready, detailsRequest, installRequest]);

  const prompted = find(promptId);
  const prompt = prompted !== null && (
    <InstallPrompt
      plugin={prompted}
      repositoryWarning={needsRepositoryWarning(prompted.source, installed)}
      onClose={() => {
        setPromptId(null);
      }}
    />
  );

  if (state.status === "loading") {
    return (
      <p role="status" className="plugin-browse-note">
        {t("loading")}
      </p>
    );
  }
  if (state.status === "failed") {
    return (
      <div className="plugin-browse">
        <p role="alert" className="settings-error">
          {state.message === "" ? t("errors.unreachable") : pluginMessageText(t, state.message)}
        </p>
        <div>
          <button
            type="button"
            className="settings-button"
            onClick={() => void loadPluginIndex(language)}
          >
            {t("retry")}
          </button>
        </div>
      </div>
    );
  }

  const notice =
    state.index.freshness === "offline"
      ? t("errors.offline")
      : state.index.notice !== null
        ? pluginMessageText(t, state.index.notice)
        : null;
  const details = find(detailsId);

  return (
    <div className="plugin-browse">
      {notice !== null && (
        <p role="status" className="plugin-browse-notice">
          {notice}
        </p>
      )}
      {details !== null ? (
        <PluginDetails
          plugin={details}
          installed={isInstalled(details.id)}
          onBack={() => {
            setDetailsId(null);
          }}
          onInstall={() => {
            setPromptId(details.id);
          }}
        />
      ) : (
        <>
          <div className="plugin-browse-filters">
            <label htmlFor={searchId} className="visually-hidden">
              {t("searchLabel")}
            </label>
            <input
              id={searchId}
              type="search"
              className="settings-input plugin-browse-search"
              placeholder={t("searchPlaceholder")}
              value={search}
              onChange={(event) => {
                setSearch(event.target.value);
              }}
            />
            <div
              role="group"
              aria-label={t("categoriesLabel")}
              className="plugin-browse-categories"
            >
              {(["all", ...CATEGORIES] as const).map((value) => (
                <button
                  key={value}
                  type="button"
                  className="settings-button"
                  aria-pressed={category === value}
                  onClick={() => {
                    setCategory(value);
                  }}
                >
                  {t(`category.${value}`)}
                </button>
              ))}
            </div>
          </div>
          <BrowseList
            plugins={plugins}
            visible={visiblePlugins(plugins, category, search)}
            isInstalled={isInstalled}
            onDetails={setDetailsId}
            onInstall={setPromptId}
          />
          <p className="plugin-browse-footer">{t("footer")}</p>
        </>
      )}
      {prompt}
    </div>
  );
}

function BrowseList({
  plugins,
  visible,
  isInstalled,
  onDetails,
  onInstall,
}: {
  plugins: readonly IndexedPlugin[];
  visible: readonly IndexedPlugin[];
  isInstalled: (id: string) => boolean;
  onDetails: (id: string) => void;
  onInstall: (id: string) => void;
}) {
  const { t } = useTranslation("pluginBrowse");
  if (plugins.length === 0) return <p className="plugin-browse-note">{t("empty")}</p>;
  if (visible.length === 0) return <p className="plugin-browse-note">{t("noMatch")}</p>;
  return (
    <ul className="plugin-browse-list" aria-label={t("listLabel")}>
      {visible.map((plugin) => (
        <PluginCard
          key={`${plugin.source.kind === "repo" ? String(plugin.source.repositoryId) : "official"}:${plugin.id}`}
          plugin={plugin}
          installed={isInstalled(plugin.id)}
          onDetails={() => {
            onDetails(plugin.id);
          }}
          onInstall={() => {
            onInstall(plugin.id);
          }}
        />
      ))}
    </ul>
  );
}
