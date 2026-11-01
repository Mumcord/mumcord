import { useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Group } from "../settings/components/Section";
import { setAllPluginsOff, usePlugins } from "./pluginStore";
import "./plugins.css";

const PLUGIN_TABS = ["installed", "browse", "updates", "repositories", "developer"] as const;
type PluginTab = (typeof PLUGIN_TABS)[number];

/**
 * The content of each tab. The tabs are built by their own tasks; each one replaces its entry
 * here with its root component.
 */
const TAB_CONTENT: Record<PluginTab, ReactNode> = {
  installed: null,
  browse: null,
  updates: null,
  repositories: null,
  developer: null,
};

/**
 * Settings → Plugins (FR-6.58): header with "Turn off all plugins", the `--no-plugins` banner and
 * the tabs. The Developer tab only exists while Developer mode is on (FR-6.73).
 */
export function PluginsSection({ developerMode }: { developerMode: boolean }) {
  const { t } = useTranslation("pluginSettings");
  const { overview, loadFailed } = usePlugins();
  const baseId = useId();
  const [selected, setSelected] = useState<PluginTab>("installed");
  const tabRefs = useRef(new Map<PluginTab, HTMLButtonElement>());
  const tabs = PLUGIN_TABS.filter((tab) => tab !== "developer" || developerMode);
  // Turning Developer mode off while its tab is open falls back to the first tab.
  const active = tabs.includes(selected) ? selected : "installed";
  const allOff = overview?.allOff ?? false;
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
    <Group
      title={t("title")}
      icon="puzzle"
      wide
      actions={
        <button
          type="button"
          className="settings-button"
          disabled={overview === null}
          onClick={() => void setAllPluginsOff(!allOff)}
        >
          {allOff ? t("turnOnAll") : t("turnOffAll")}
        </button>
      }
    >
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
          </button>
        ))}
      </div>
      <div role="tabpanel" id={panelId(active)} aria-labelledby={tabId(active)} tabIndex={-1}>
        {TAB_CONTENT[active]}
      </div>
    </Group>
  );
}
