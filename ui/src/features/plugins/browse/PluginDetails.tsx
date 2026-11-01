import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { IndexedPlugin } from "../../../api";
import { isPermissionId, PERMISSION_RISK, permissionTextKey } from "../permissions/catalogue";
import { formatSize } from "../updates/formatSize";
import { PluginIcon } from "./PluginIcon";
import { sourceLabel } from "./source";
import { useIndexImage } from "./useIndexImage";
import "../permissions/permissions.css";
import "./details.css";

export interface PluginDetailsProps {
  plugin: IndexedPlugin;
  installed: boolean;
  onBack: () => void;
  onInstall: () => void;
}

/** A plugin's store page: what it is, who made it and which permissions it may ask for. */
export function PluginDetails({ plugin, installed, onBack, onInstall }: PluginDetailsProps) {
  const { t, i18n } = useTranslation("pluginBrowse");
  const { t: tTabs } = useTranslation("pluginSettings");
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    heading.current?.focus();
  }, [plugin.id]);
  const name = plugin.name;

  const official = plugin.source.kind === "official";
  const repository = plugin.source.kind === "repo" ? plugin.source.name : null;
  const meta = [
    plugin.authors.length > 0 ? t("by", { authors: plugin.authors.join(", ") }) : null,
    plugin.version,
    plugin.size !== null ? formatSize(plugin.size, i18n.language) : null,
    t("updatedOn", { date: formatDate(plugin.updated, i18n.language) }),
    plugin.categories.length > 0
      ? plugin.categories.map((c) => t(`category.${c}`, { defaultValue: c })).join(", ")
      : null,
  ].filter((part) => part !== null);

  return (
    <article className="pd" aria-labelledby={`plugin-details-${plugin.id}`}>
      <button type="button" className="pd-back" onClick={onBack}>
        <span aria-hidden="true">← </span>
        {tTabs("tabs.browse")}
      </button>
      <header className="pd-header">
        <PluginIcon id={plugin.id} name={name} hasIcon={plugin.icon !== null} />
        <div className="pd-headline">
          <div className="pd-title-row">
            <h3 id={`plugin-details-${plugin.id}`} ref={heading} tabIndex={-1}>
              {name}
            </h3>
            <span className={official ? "pd-pill" : "pd-pill pd-pill-unofficial"}>
              <span aria-hidden="true">{official ? "✓ " : "⚠ "}</span>
              {official
                ? plugin.reviewed
                  ? t("officialReviewed")
                  : t("official")
                : sourceLabel(t, plugin.source)}
            </span>
          </div>
          <p className="pd-meta">{meta.join(" · ")}</p>
          <p className="pd-summary">{plugin.summary}</p>
        </div>
        <div className="pd-action">
          {installed ? (
            <span className="plugin-label">{t("installed")}</span>
          ) : (
            <button
              type="button"
              className={official ? "settings-button settings-button--primary" : "settings-button"}
              aria-label={t("installNamed", { name })}
              onClick={onInstall}
            >
              {official ? t("install") : `${t("install")}…`}
            </button>
          )}
        </div>
      </header>
      {plugin.screenshots.length > 0 && (
        <section aria-label={t("screenshots")} className="pd-shots">
          {plugin.screenshots.map((shot, index) => (
            <Screenshot key={index} id={plugin.id} index={index} alt={shot.alt} />
          ))}
        </section>
      )}
      {repository !== null && (
        <div className="pd-banner" role="note">
          <span className="pd-banner-icon" aria-hidden="true">
            ⚠
          </span>
          <div>
            <strong>{t("unofficialTitle")}</strong>
            <p>{t("unofficialText", { repository })}</p>
          </div>
        </div>
      )}
      <div className="pd-columns">
        {plugin.permissions.length > 0 && (
          <section className="pd-card" aria-labelledby="pd-can-do">
            <h4 id="pd-can-do" className="pd-card-title">
              {t("whatItCanDo")}
            </h4>
            <PermissionPreview ids={plugin.permissions} />
          </section>
        )}
        <div className="pd-side">
          {official && (
            <section className="pd-card" aria-labelledby="pd-never">
              <h4 id="pd-never" className="pd-card-title">
                {t("neverTitle")}
              </h4>
              <p>{t("neverText")}</p>
            </section>
          )}
          <section className="pd-card" aria-labelledby="pd-details">
            <h4 id="pd-details" className="pd-card-title">
              {t("details")}
            </h4>
            <ul className="pd-facts">
              {repository !== null && <li>{t("repositoryName", { repository })}</li>}
              {plugin.sourceUrl !== null && (
                <li>{t("factSource", { url: plugin.sourceUrl.replace(/^https?:\/\//, "") })}</li>
              )}
              {plugin.license !== null && <li>{t("factLicence", { license: plugin.license })}</li>}
              {plugin.minMumcord !== null && (
                <li>{t("factMinVersion", { version: plugin.minMumcord })}</li>
              )}
              {plugin.signingKey !== null && (
                <li data-tooltip={plugin.signingKey}>
                  {t("factSigned", { key: shortKey(plugin.signingKey) })}
                </li>
              )}
              {official && plugin.reviewed && plugin.reviewedOn === null && (
                <li>{t("reviewed")}</li>
              )}
              {plugin.reviewedOn !== null && (
                <li>{t("factReviewed", { date: formatDate(plugin.reviewedOn, i18n.language) })}</li>
              )}
              {plugin.addedOn !== null && (
                <li>{t("factAdded", { date: formatDate(plugin.addedOn, i18n.language) })}</li>
              )}
              {repository !== null && <li>{t("updatesFrom")}</li>}
            </ul>
          </section>
          {repository !== null && (
            <section className="pd-card" aria-labelledby="pd-checks">
              <h4 id="pd-checks" className="pd-card-title">
                {t("checksTitle")}
              </h4>
              <p>{t("checksText")}</p>
            </section>
          )}
        </div>
      </div>
    </article>
  );
}

function Screenshot({ id, index, alt }: { id: string; index: number; alt: string }) {
  const url = useIndexImage(id, { kind: "screenshot", index });
  return url === null ? null : <img className="pd-shot" src={url} alt={alt} />;
}

/** `1CB49831E26E61AD` → `1CB4…61AD`, as in the mockup; the full id is the tooltip. */
function shortKey(key: string): string {
  return key.length > 8 ? `${key.slice(0, 4)}…${key.slice(-4)}` : key;
}

function formatDate(value: string, language: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime())
    ? value
    : new Intl.DateTimeFormat(language, { dateStyle: "medium" }).format(date);
}

/**
 * Every permission the listed version may ask for. Which are required is only known from the
 * package, so the install prompt shows the split.
 */
function PermissionPreview({ ids }: { ids: readonly string[] }) {
  const { t } = useTranslation("pluginPermissions");
  return (
    <ul className="pd-perms">
      {ids.map((id) => {
        const known = isPermissionId(id);
        const risk = known ? PERMISSION_RISK[id] : "red";
        return (
          <li key={id} className="pd-perm">
            <span className={`pd-perm-icon perm-risk perm-risk-${risk}`} aria-hidden="true">
              {risk === "red" ? "▲" : "●"}
            </span>
            <span className="pd-perm-body">
              <strong>{known ? t(`texts.${permissionTextKey(id)}`) : id}</strong>
              {known && <span>{t(`explanations.${permissionTextKey(id)}`)}</span>}
            </span>
            {risk === "red" && <span className="perm-risk-badge">{t("risk.red")}</span>}
          </li>
        );
      })}
    </ul>
  );
}
