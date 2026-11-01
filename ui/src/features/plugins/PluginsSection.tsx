import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { type PluginUpdates } from "../../api";
import { Switch } from "../settings/components/Switch";
import { consumeRequestedPluginTab, useRequestedPluginTab } from "./pluginRequestStore";
import { setAllPluginsOff, usePlugins } from "./pluginStore";
import { RepositoriesTab, type MirrorSetting } from "./repos";
import { UpdatesTab } from "./updates";
import { useUpdatesCount } from "./updates/useUpdatesCount";
import "./plugins.css";
import { BrowseTab } from "./browse/BrowseTab";
import { DeveloperTab } from "./developer/DeveloperTab";
import { InstalledTab } from "./installed/InstalledTab";

const PLUGIN_TABS = ["installed", "browse", "updates", "repositories", "developer"] as const;
type PluginTab = (typeof PLUGIN_TABS)[number];

/**
 * The content of each tab. The tabs are built by their own tasks; each one replaces its entry
 * here with its root component.
 */
const TAB_CONTENT: Record<PluginTab, ReactNode> = {
  installed: <InstalledTab />,
  browse: <BrowseTab />,
  updates: null,
  repositories: <RepositoriesTab />,
  developer: <DeveloperTab />,
};

/**
 * Settings → Plugins (FR-6.58): the tabs with their counts, the `--no-plugins` banner and the
 * "Turn off all plugins" switch in the footer of the Installed tab. The page title and sandbox
 * subtitle come from the settings shell. The Developer tab only exists while Developer mode is on (FR-6.73).
 */
export function PluginsSection({
  developerMode,
  mirror,
  updateMode = "auto",
  onUpdateModeChange = () => undefined,
}: {
  developerMode: boolean;
  /** The optional mirror of the official list, shown under Repositories → Advanced. */
  mirror?: MirrorSetting | undefined;
  /** The "Plugin updates" setting (FR-6.66). */
  updateMode?: PluginUpdates;
  onUpdateModeChange?: (mode: PluginUpdates) => void;
}) {
  const { t } = useTranslation("pluginSettings");
  const { overview, loadFailed } = usePlugins();
  const baseId = useId();
  const [selected, setSelected] = useState<PluginTab>("installed");
  const requestedTab = useRequestedPluginTab();
  // A request (e.g. from the notification centre) switches the tab once, then is consumed.
  if (requestedTab !== null && requestedTab !== selected) setSelected(requestedTab);
  useEffect(() => {
    if (requestedTab !== null) consumeRequestedPluginTab();
  }, [requestedTab]);
  const tabRefs = useRef(new Map<PluginTab, HTMLButtonElement>());
  const tabs = PLUGIN_TABS.filter((tab) => tab !== "developer" || developerMode);
  // Turning Developer mode off while its tab is open falls back to the first tab.
  const active = tabs.includes(selected) ? selected : "installed";
  const allOff = overview?.allOff ?? false;
  const installedCount = overview?.plugins.length;
  const updatesCount = useUpdatesCount();
  const tabId = (tab: PluginTab) => `${baseId}-tab-${tab}`;
  const panelId = (tab: PluginTab) => `${baseId}-panel-${tab}`;

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    const index = tabs.indexOf(active);
    const next =
      event.key === "ArrowRight" || event.key === "ArrowDown"
        ? tabs[(index + 1) % tabs.length]
        : event.key === "ArrowLeft" || event.key === "ArrowUp"
          ? tabs[(index - 1 + tabs.length) % tabs.length]
          : event.key === "Home"
            ? tabs[0]
            : event.key === "End"
              ? tabs[tabs.length - 1]
              : undefined;
    if (next === undefined) return;
    event.preventDefault();
    setSelected(next);
    tabRefs.current.get(next)?.focus();
  };

  return (
    <div className="plugins-section">
      {overview?.noPlugins === true && (
        <p role="status" className="plugins-banner">
          {t("noPlugins")}
        </p>
      )}
      {allOff && overview?.noPlugins !== true && (
        <p role="status" className="plugins-banner">
          {t("allOff")}
        </p>
      )}
      {loadFailed && (
        <p role="alert" className="settings-error">
          {t("loadFailed")}
        </p>
      )}
      <div role="tablist" aria-label={t("tabsLabel")} className="plugins-tabs">
        {tabs.map((tab) => (
          <button
            key={tab}
            type="button"
            role="tab"
            id={tabId(tab)}
            aria-selected={tab === active}
            aria-controls={panelId(tab)}
            tabIndex={tab === active ? 0 : -1}
            className="plugins-tab"
            onKeyDown={onKeyDown}
            ref={(element) => {
              if (element) tabRefs.current.set(tab, element);
              else tabRefs.current.delete(tab);
            }}
            onClick={() => {
              setSelected(tab);
            }}
          >
            {t(`tabs.${tab}`)}
            {tab === "installed" && installedCount !== undefined && (
              <span className="plugins-tab__count">{installedCount}</span>
            )}
            {tab === "updates" && updatesCount > 0 && (
              <span className="plugins-tab__pill">{updatesCount}</span>
            )}
          </button>
        ))}
      </div>
      <div role="tabpanel" id={panelId(active)} aria-labelledby={tabId(active)} tabIndex={-1}>
        {active === "updates" ? (
          <UpdatesTab mode={updateMode} onModeChange={onUpdateModeChange} />
        ) : active === "repositories" ? (
          <RepositoriesTab mirror={mirror} />
        ) : (
          TAB_CONTENT[active]
        )}
      </div>
      {active === "installed" && (
        <div className="plugins-footer">
          <Switch
            label={t("turnOffAll")}
            checked={allOff}
            disabled={overview === null}
            onChange={(checked) => void setAllPluginsOff(checked)}
          />
        </div>
      )}
    </div>
  );
}
