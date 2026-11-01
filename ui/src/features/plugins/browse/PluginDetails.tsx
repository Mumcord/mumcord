import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { IndexedPlugin } from "../../../api";
import { isPermissionId, PERMISSION_RISK, permissionTextKey } from "../permissions/catalogue";
import { PluginIcon } from "./PluginIcon";
import { sourceLabel } from "./source";
import { useIndexImage } from "./useIndexImage";
import "../permissions/permissions.css";

export interface PluginDetailsProps {
  plugin: IndexedPlugin;
  installed: boolean;
  onBack: () => void;
  onInstall: () => void;
}

/** A plugin's store page: what it is, who made it and which permissions it may ask for. */
export function PluginDetails({ plugin, installed, onBack, onInstall }: PluginDetailsProps) {
  const { t } = useTranslation("pluginBrowse");
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    heading.current?.focus();
  }, [plugin.id]);
  const name = plugin.name;

  return (
    <article className="plugin-details" aria-labelledby={`plugin-details-${plugin.id}`}>
      <div>
        <button type="button" className="settings-button" onClick={onBack}>
          {t("back")}
        </button>
      </div>
      <header className="plugin-details-header">
        <PluginIcon id={plugin.id} name={name} hasIcon={plugin.icon !== null} />
        <div>
          <h3 id={`plugin-details-${plugin.id}`} ref={heading} tabIndex={-1}>
            {name}
          </h3>
          <p className="plugin-details-meta">
            {t("version", { version: plugin.version })}
            {plugin.authors.length > 0 && <> · {t("by", { authors: plugin.authors.join(", ") })}</>}
            {plugin.size !== null && (
              <> · {t("size", { size: Math.max(1, Math.round(plugin.size / 1024)) })}</>
            )}
          </p>
          <div className="plugin-card-labels">
            <span
              className={
                plugin.source.kind === "official"
                  ? "plugin-label"
                  : "plugin-label plugin-label-unofficial"
              }
            >
              {sourceLabel(t, plugin.source)}
            </span>
            {plugin.reviewed && <span className="plugin-label">{t("reviewed")}</span>}
            {plugin.categories.map((category) => (
              <span key={category} className="plugin-label">
                {t(`category.${category}`, { defaultValue: category })}
              </span>
            ))}
          </div>
        </div>
      </header>
      <p>{plugin.summary}</p>
      {plugin.screenshots.length > 0 && (
        <section aria-label={t("screenshots")} className="plugin-details-shots">
          {plugin.screenshots.map((shot, index) => (
            <Screenshot key={index} id={plugin.id} index={index} alt={shot.alt} />
          ))}
        </section>
      )}
      {plugin.permissions.length > 0 && (
        <section>
          <h4>{t("mayAskFor")}</h4>
          <PermissionPreview ids={plugin.permissions} />
        </section>
      )}
      <div>
        {installed ? (
          <span className="plugin-label">{t("installed")}</span>
        ) : (
          <button
            type="button"
            className="settings-button settings-button--primary"
            onClick={onInstall}
          >
            {t("installNamed", { name })}
          </button>
        )}
      </div>
    </article>
  );
}

function Screenshot({ id, index, alt }: { id: string; index: number; alt: string }) {
  const url = useIndexImage(id, { kind: "screenshot", index });
  return url === null ? null : <img className="plugin-details-shot" src={url} alt={alt} />;
}

/**
 * Every permission the listed version may ask for. Which are required is only known from the
 * package, so the install prompt shows the split.
 */
function PermissionPreview({ ids }: { ids: readonly string[] }) {
  const { t } = useTranslation("pluginPermissions");
  return (
    <ul className="plugin-details-permissions">
      {ids.map((id) => {
        const known = isPermissionId(id);
        const risk = known ? PERMISSION_RISK[id] : "red";
        return (
          <li key={id}>
            <span className={`perm-risk perm-risk-${risk}`} aria-hidden="true">
              {risk === "red" ? "▲" : "●"}
            </span>{" "}
            {known ? t(`texts.${permissionTextKey(id)}`) : id}
            {risk === "red" && <span className="perm-risk-badge">{t("risk.red")}</span>}
          </li>
        );
      })}
    </ul>
  );
}
