import { useEffect, useId, useRef, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { InstallProgress } from "../prompt/InstallProgress";
import type { IndexedPlugin } from "../../../api";
import type { PluginScored } from "../../../bindings/commands";
import { isPermissionId, PERMISSION_RISK, permissionTextKey } from "../permissions/catalogue";
import { openScoreDoc, ScoreBreakdown, ScoreChip, ScoreMeter, type ScoreFacts } from "../score";
import { formatSize } from "../updates/formatSize";
import { usePluginScore } from "./usePluginScore";
import { PluginIcon } from "./PluginIcon";
import { sourceLabel } from "./source";
import { useIndexImage } from "./useIndexImage";
import { usePermissionExplanation } from "../permissions/usePermissionExplanation";
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
  const { t: tOrigins } = useTranslation("pluginOrigins");
  const name = plugin.name;
  const scored = usePluginScore(
    plugin.id,
    plugin.version,
    "listing",
    plugin.source.kind === "official",
  );

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
            {scored !== null && scored.kind !== "official" && <ScoreChip scored={scored} />}
          </div>
          <p className="pd-meta">{meta.join(" · ")}</p>
          <p className="pd-summary">{plugin.summary}</p>
          <InstallProgress id={plugin.id} name={plugin.name} />
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
      <PluginScoreCard
        name={name}
        scored={scored}
        official={official}
        facts={{
          websites: plugin.origins.length,
          ...(plugin.size !== null ? { packageSize: formatSize(plugin.size, i18n.language) } : {}),
        }}
      />
      <div className="pd-columns">
        {plugin.permissions.length > 0 && (
          <section className="pd-card" aria-labelledby="pd-can-do">
            <h4 id="pd-can-do" className="pd-card-title">
              {t("whatItCanDo")}
            </h4>
            <PermissionPreview ids={plugin.permissions} pluginName={name} />
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
      <div className="pd-surfaces">
        {plugin.origins.length > 0 && (
          <DetailSection title={tOrigins("details.title")}>
            <p className="pd-hint">{tOrigins("intro", { plugin: name })}</p>
            <ul className="ps-origins" aria-label={tOrigins("listLabel")}>
              {plugin.origins.map((origin) => (
                <li key={origin} className="ps-origin">
                  <code>{origin}</code>
                </li>
              ))}
            </ul>
          </DetailSection>
        )}
        <PluginPlaces permissions={plugin.permissions} />
        <OwnCode
          permissions={plugin.permissions}
          relay={plugin.permissions.includes(RELAY) ? <>{tOrigins("details.relayAsks")}</> : null}
        />
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
function PermissionPreview({ ids, pluginName }: { ids: readonly string[]; pluginName: string }) {
  const { t } = useTranslation("pluginPermissions");
  const explain = usePermissionExplanation();
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
              {known && <span>{explain(id, pluginName)}</span>}
            </span>
            {risk === "red" && <span className="perm-risk-badge">{t("risk.red")}</span>}
          </li>
        );
      })}
    </ul>
  );
}

const RELAY = "peers.relay";
const WEBVIEW = "ui.webview";

/** The places a plugin can show something, in the order of the mockup. */
const PLACES: readonly { permission: string; key: string }[] = [
  { permission: "ui.stage", key: "stage" },
  { permission: "ui.side-panel", key: "side-panel" },
  { permission: "ui.dialog", key: "dialog" },
  { permission: "ui.panel", key: "panel" },
  { permission: "ui.app-menu", key: "appMenu" },
  { permission: "ui.chat-card", key: "chat" },
];

/** A titled block of the details page; shared by Browse and Installed. */
export function DetailSection({ title, children }: { title: string; children: ReactNode }) {
  const id = useId();
  return (
    <section className="pd-section" aria-labelledby={id}>
      <h4 id={id} className="pd-card-title">
        {title}
      </h4>
      {children}
    </section>
  );
}

/** "Where it shows" (FR-9.30): one chip per surface the plugin's permissions give it. */
export function PluginPlaces({
  permissions,
  appMenuItems = 0,
}: {
  permissions: readonly string[];
  /** How many Apps-menu items the installed plugin declares; the count is shown from two on. */
  appMenuItems?: number;
}) {
  const { t } = useTranslation("pluginOrigins");
  const places = PLACES.filter((place) => permissions.includes(place.permission));
  if (places.length === 0) return null;
  return (
    <DetailSection title={t("details.wherePlace")}>
      <ul className="ps-places">
        {places.map((place) => (
          <li key={place.key} className="ps-chip">
            {place.key === "appMenu"
              ? appMenuItems > 1
                ? t("details.places.appMenu", { count: appMenuItems })
                : t("details.tags.appMenu")
              : t(`details.places.${place.key}`)}
          </li>
        ))}
      </ul>
    </DetailSection>
  );
}

/**
 * "Own code" and "Relay data": what the sealed page can and cannot touch. The row only appears for
 * a plugin that has its own web page or asks for the relay.
 */
export function OwnCode({
  permissions,
  relay,
  webSize = null,
}: {
  permissions: readonly string[];
  /** Size in bytes of the plugin's web pages, when known (installed plugins). */
  webSize?: number | null;
  /** The content of the "Relay data" row; `null` leaves the row out. */
  relay: ReactNode;
}) {
  const { t, i18n } = useTranslation("pluginOrigins");
  const webview = permissions.includes(WEBVIEW);
  if (!webview && relay === null) return null;
  return (
    <DetailSection title={t("details.ownCode")}>
      <dl className="ps-facts">
        {webview && (
          <>
            <dt>{t("details.sealedPage")}</dt>
            <dd>
              {webSize === null
                ? t("details.sealedPageTextNoSize", { entry: "web/index.html" })
                : t("details.sealedPageText", {
                    entry: "web/index.html",
                    size: formatSize(webSize, i18n.language),
                  })}
            </dd>
          </>
        )}
        {relay !== null && (
          <>
            <dt>{t("details.relayData")}</dt>
            <dd>{relay}</dd>
          </>
        )}
      </dl>
    </DetailSection>
  );
}

export interface PluginScoreCardProps {
  name: string;
  /** From `usePluginScore`; `null` draws nothing. */
  scored: PluginScored | null;
  /** Official plugins show the review instead of a number (mockup 06). */
  official: boolean;
  /** Developer plugins are scored for information only. */
  developer?: boolean;
  facts?: ScoreFacts;
}

/** The score card of the details page (score mockups 02 and 06); the engine does all the maths. */
export function PluginScoreCard({
  name,
  scored,
  official,
  developer = false,
  facts,
}: PluginScoreCardProps) {
  const { t } = useTranslation("pluginScore");
  const { t: tOrigins } = useTranslation("pluginOrigins");
  const titleId = useId();

  if (official) {
    return (
      <section className="pd-score" data-level="official" aria-labelledby={titleId}>
        <h4 id={titleId} className="pd-card-title">
          {tOrigins("details.securityTitle")}
        </h4>
        <div className="pd-score-official">
          <ScoreChip scored={{ kind: "official" }} size="md" />
          <p className="pd-score-lead">{t("officialNoScore")}</p>
        </div>
      </section>
    );
  }
  if (scored === null) return null;
  if (scored.kind !== "scored") {
    return (
      <section className="pd-score" data-level="high" aria-labelledby={titleId}>
        <h4 id={titleId} className="pd-card-title">
          {t("title")}
        </h4>
        <ScoreChip scored={scored} size="md" />
      </section>
    );
  }
  const { value, band } = scored.score;
  return (
    <section className="pd-score" data-level={band} aria-labelledby={titleId}>
      <h4 id={titleId} className="pd-card-title">
        {t("title")}
      </h4>
      <div className="pd-score-top">
        <div className="pd-score-big" aria-hidden="true">
          <span className="pd-score-num">{value}</span>
          <span className="pd-score-of">{t("outOf")}</span>
        </div>
        <div className="pd-score-gauge">
          <ScoreChip scored={scored} developer={developer} size="md" />
          <ScoreMeter value={value} band={band} />
        </div>
        <p className="pd-score-lead">
          <strong>{t("higherMeansMore")}</strong> {t("explain")}
        </p>
      </div>
      {developer && <p className="pd-score-note">{t("developerNote")}</p>}
      <p className="pd-score-note">
        <span aria-hidden="true">ⓘ</span>
        <span>{t("honesty")}</span>
      </p>
      <ScoreBreakdown
        score={scored.score}
        pluginName={name}
        {...(facts !== undefined ? { facts } : {})}
        collapsible
        onHowCalculated={openScoreDoc}
      />
    </section>
  );
}
