import { useState, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { usePluginIconUrl } from "../render/assets";
import { activateAppMenuItem, MENU_LIMIT, useAppMenu, type AppMenuEntry } from "./useAppMenu";
import "./appmenu.css";

/** A stable hue (0-359) per plugin id, so a plugin keeps its tile colour everywhere. */
function pluginHue(pluginId: string): number {
  let hash = 2166136261;
  for (const char of pluginId) {
    hash = Math.imul(hash ^ (char.codePointAt(0) ?? 0), 16777619) >>> 0;
  }
  return hash % 360;
}

function EntryIcon({ entry }: { entry: AppMenuEntry }) {
  const url = usePluginIconUrl(entry.plugin, entry.icon);
  return url === null ? (
    <span
      className="appmenu__icon appmenu__icon--tile"
      style={{ "--appmenu-hue": pluginHue(entry.plugin) } as CSSProperties}
      aria-hidden="true"
    >
      {Array.from(entry.pluginName)[0]?.toUpperCase()}
    </span>
  ) : (
    <img className="appmenu__icon" src={url} alt="" width={20} height={20} />
  );
}

function Entry({ entry, onChosen }: { entry: AppMenuEntry; onChosen: () => void }) {
  const { t } = useTranslation("pluginAppMenu");
  const subline = t(entry.official ? "sublineOfficial" : "subline", { plugin: entry.pluginName });
  const disabled = !entry.enabled;
  return (
    <button
      type="button"
      role="menuitem"
      tabIndex={-1}
      className="server-menu__item chat-apps__item appmenu__item"
      aria-disabled={disabled || undefined}
      onClick={() => {
        if (disabled) return;
        onChosen();
        activateAppMenuItem(entry);
      }}
    >
      <EntryIcon entry={entry} />
      <span className="appmenu__text">
        {entry.label}
        <span className="appmenu__sub">{subline}</span>
        {disabled && entry.reason !== null && (
          <span className="chat-apps__reason">{entry.reason}</span>
        )}
      </span>
    </button>
  );
}

/**
 * The "Plugins" group of the Apps menu (FR-9.53, mockup 06): the first eight items, the rest
 * behind "More plugins…", and the footer. Renders nothing while no plugin offers an item.
 * Labels and reasons come from plugins, so they are only ever shown as plain text.
 */
export function PluginMenuItems({ onChosen }: { onChosen: () => void }) {
  const { t } = useTranslation("pluginAppMenu");
  const entries = useAppMenu();
  const [more, setMore] = useState(false);
  if (entries.length === 0) return null;
  const shown = more ? entries : entries.slice(0, MENU_LIMIT);
  const hidden = entries.length - MENU_LIMIT;
  return (
    <>
      <div className="server-menu__separator" role="separator" />
      <p className="appmenu__group" aria-hidden="true">
        {t("pluginsGroup")}
      </p>
      <div role="group" aria-label={t("pluginsGroup")}>
        {shown.map((entry) => (
          <Entry key={`${entry.plugin}/${entry.item}`} entry={entry} onChosen={onChosen} />
        ))}
        {hidden > 0 && !more && (
          <button
            type="button"
            role="menuitem"
            tabIndex={-1}
            className="server-menu__item chat-apps__item appmenu__item"
            aria-expanded={false}
            onClick={() => {
              setMore(true);
            }}
          >
            <span className="appmenu__text">{t("moreItems")}</span>
          </button>
        )}
      </div>
      <div className="server-menu__separator" role="separator" />
      <p className="appmenu__foot">{t("footer")}</p>
    </>
  );
}
