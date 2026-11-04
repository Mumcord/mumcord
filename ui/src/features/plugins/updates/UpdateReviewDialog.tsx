import { useEffect, useId, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type PluginUpdate, type PluginUpdateReview } from "../../../api";
import { ModalDialog } from "../../settings/components/ModalDialog";
import { usePermissionText } from "../permissions/usePermissionText";
import "../permissions/permissions.css";
import { EffectText, ScoreChip, ScoreMeter } from "../score";
import { newPermissionsOf, offDomainOrigins, scoredOf } from "./reviewData";

/** Versions the score looks back on for a recently added permission (term H of the spec 09 score). */
const RECENT_VERSIONS = 5;

/**
 * The update review (FR-9.83): new websites marked "New" (surfaces mockup 09), the score before and
 * after with what the new version asks for, and the tick an update to High needs (score mockup 04b).
 */
export function UpdateReviewDialog({
  update,
  icon,
  optional,
  onClose,
  onDone,
}: {
  update: PluginUpdate;
  icon: ReactNode;
  /** The optional permissions that are on now; they stay on, new ones are never granted. */
  optional: string[];
  onClose: () => void;
  /** The update was installed or skipped: the list changes. */
  onDone: () => void;
}) {
  const { t } = useTranslation("pluginUpdates");
  const { t: tBrowse } = useTranslation("pluginBrowse");
  const { t: tPerm } = useTranslation("pluginPermissions");
  const permissionText = usePermissionText();
  const tickId = useId();
  // The listing carries no purposes for new websites; the prepared review reads them from the
  // verified new package. Until it arrives (or if it fails) the listing's review is shown.
  const [prepared, setPrepared] = useState<PluginUpdateReview | null>(null);
  const [preparing, setPreparing] = useState(true);
  useEffect(() => {
    let cancelled = false;
    getApi()
      .pluginUpdateReviewPrepare(update.id, update.to)
      .then((result) => {
        if (!cancelled) setPrepared(result);
      })
      .catch((error: unknown) => {
        console.error("Preparing the plugin update review failed", error);
      })
      .finally(() => {
        if (!cancelled) setPreparing(false);
      });
    return () => {
      cancelled = true;
    };
  }, [update.id, update.to]);
  const review = prepared ?? update.review;
  const [ticked, setTicked] = useState(review.highRiskConfirmed);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);

  const newOrigins = review.origins.filter((origin) => origin.new).length;
  const offDomain = offDomainOrigins(review);
  const newPermissions = newPermissionsOf(update);
  const risk = review.risk;
  const riskUp = risk !== null && risk.toValue > risk.fromValue;
  const highRisk = review.needsHighRiskConfirm;
  const effects = new Map(review.permissionEffects.map((p) => [p.permission, p.points]));
  const permissionPoints = review.permissionEffects.reduce((sum, p) => sum + p.points, 0);

  const source =
    update.source.kind === "repo" ? (
      <span className="plugin-review__chip plugin-review__chip--unofficial">
        {tBrowse("unofficial", { repository: update.source.name })}
      </span>
    ) : update.source.kind === "official" ? (
      <span className="plugin-review__chip">{tBrowse("official")}</span>
    ) : null;
  const subtitle = (
    <>
      {source !== null && <>{source} · </>}
      {t("dialog.now", { version: update.from })} ·{" "}
      {update.approval?.kind === "newAuthorKey" ? t("dialog.newKey") : t("dialog.sameKey")}
    </>
  );

  const install = async () => {
    setBusy(true);
    setFailed(false);
    try {
      // The engine refuses an update to High until the tick is recorded for this version.
      if (highRisk && !review.highRiskConfirmed) {
        await getApi().pluginConfirmUpdateHighRisk(update.id, update.to);
      }
      await getApi().pluginApplyUpdate(update.id, optional);
      onDone();
    } catch (error: unknown) {
      console.error("Installing a reviewed plugin update failed", error);
      setFailed(true);
      setBusy(false);
    }
  };

  const skip = async () => {
    setBusy(true);
    setFailed(false);
    try {
      await getApi().pluginSkipUpdateVersion(update.id, update.to);
      onDone();
    } catch (error: unknown) {
      console.error("Skipping a plugin update failed", error);
      setFailed(true);
      setBusy(false);
    }
  };

  return (
    <ModalDialog
      title={t("dialog.title", { name: update.name, version: update.to })}
      onClose={onClose}
      className="plugin-review"
      icon={icon}
      subtitle={<p className="plugin-review__subtitle">{subtitle}</p>}
    >
      {newOrigins > 0 && (
        <div className="plugin-review__alert">
          <span className="plugin-review__alert-mark" aria-hidden="true">
            ⚠
          </span>
          <p>
            <strong>{t("dialog.newWebsites", { count: newOrigins })}</strong>{" "}
            {t("dialog.staysOn", { name: update.name, version: update.from })}
            {review.otherDomain && <> {t("dialog.otherDomain")}</>}
          </p>
        </div>
      )}

      {riskUp && (
        <div className="plugin-review__scores">
          <div className="plugin-review__score">
            <span className="plugin-review__score-label">
              {t("dialog.scoreNow", { version: update.from })}
            </span>
            <ScoreChip scored={scoredOf(risk.fromValue, risk.fromBand)} />
            <ScoreMeter value={risk.fromValue} band={risk.fromBand} label={t("dialog.meterNow")} />
          </div>
          <span className="plugin-review__score-arrow" aria-hidden="true">
            →
          </span>
          <div className="plugin-review__score">
            <span className="plugin-review__score-label">
              {t("dialog.scoreAfter", { version: update.to })}
            </span>
            <ScoreChip scored={scoredOf(risk.toValue, risk.toBand)} />
            <ScoreMeter value={risk.toValue} band={risk.toBand} label={t("dialog.meterAfter")} />
          </div>
        </div>
      )}

      {review.origins.length > 0 && (
        <div className="plugin-review__websites">
          <span className="plugin-review__websites-icon" aria-hidden="true">
            ⇄
          </span>
          <div className="plugin-review__websites-body">
            <p className="plugin-review__websites-title">
              <strong>{permissionText("net.origins")}</strong>{" "}
              <span className="perm-risk-badge">{tPerm("risk.red")}</span>
            </p>
            <p className="plugin-review__muted">
              {t("dialog.websitesLead", { name: update.name })}
            </p>
            <p>{t("dialog.websitesWarning", { name: update.name })}</p>
            <ul className="plugin-review__origins" aria-label={t("dialog.websitesLabel")}>
              {review.origins.map((origin) => (
                <li key={origin.origin} className="plugin-review__origin">
                  <code
                    className={
                      origin.new
                        ? "plugin-review__origin-url plugin-review__origin-url--new"
                        : "plugin-review__origin-url"
                    }
                  >
                    {origin.origin}
                  </code>
                  {origin.new && <span className="plugin-review__new">{t("dialog.new")}</span>}
                  {origin.purpose !== null && origin.purpose !== "" && (
                    <span className="plugin-review__purpose">
                      {offDomain.has(origin.origin) ? `“${origin.purpose}”` : origin.purpose}
                    </span>
                  )}
                </li>
              ))}
            </ul>
          </div>
          {newOrigins > 0 && <span className="plugin-review__changed">{t("dialog.changed")}</span>}
        </div>
      )}

      {newPermissions.length > 0 ? (
        <>
          <p className="plugin-review__lead">
            {riskUp ? (
              <>
                <strong>{t("dialog.riskUpBy", { count: risk.toValue - risk.fromValue })}</strong>{" "}
                {t("dialog.asksForMore")}
              </>
            ) : (
              <strong>{t("newPermissions")}</strong>
            )}
          </p>
          <ul className="plugin-review__permissions" aria-label={t("dialog.permissionsLabel")}>
            {newPermissions.map((id) => {
              const points = effects.get(id);
              return (
                <li key={id} className="plugin-review__permission">
                  <span className="plugin-review__plus" aria-hidden="true">
                    +
                  </span>
                  <span className="plugin-review__permission-text">{permissionText(id)}</span>
                  {points !== undefined && (
                    <span className="plugin-review__effect">
                      <EffectText effect={{ kind: "points", value: points }} />
                      <span aria-hidden="true"> {t("score.points")}</span>
                    </span>
                  )}
                </li>
              );
            })}
          </ul>
        </>
      ) : (
        <p className="plugin-review__muted-line">{t("dialog.noOtherPermissions")}</p>
      )}

      {riskUp && review.recentChange !== null && (
        <p className="plugin-review__note">
          {t("dialog.recentChange", {
            versions: RECENT_VERSIONS,
            recent: review.recentChange.points,
            permissions: permissionPoints,
            multiplier: (review.recentChange.trustPercent / 100).toFixed(2),
            total: risk.toValue - risk.fromValue,
          })}
        </p>
      )}

      {update.approval?.kind === "newAuthorKey" && (
        <p role="alert" className="plugin-update__note plugin-update__note--warn">
          {t("newAuthorKey")}
        </p>
      )}

      {highRisk && (
        <div className="plugin-review__high">
          <p className="plugin-review__high-title">
            <strong>{t("dialog.highTitle")}</strong>
          </p>
          <p>{t("dialog.highText", { version: update.from })}</p>
          <div className="plugin-review__tick">
            <input
              id={tickId}
              type="checkbox"
              checked={ticked}
              onChange={(event) => {
                setTicked(event.target.checked);
              }}
            />
            <label htmlFor={tickId}>{t("dialog.highTick")}</label>
          </div>
        </div>
      )}

      {failed && (
        <p role="alert" className="settings-error">
          {t("dialog.failed", { name: update.name })}
        </p>
      )}

      <div className="plugin-review__footer">
        {!highRisk && (
          <p className="plugin-review__hint">{t("dialog.skipHint", { version: update.from })}</p>
        )}
        <div className="plugin-review__buttons">
          <button
            type="button"
            className="settings-button"
            disabled={busy}
            onClick={highRisk ? onClose : () => void skip()}
          >
            {highRisk ? t("dialog.notNow") : t("dialog.skip")}
          </button>
          <button
            type="button"
            className="settings-button settings-button--danger"
            disabled={busy || preparing || (highRisk && !ticked)}
            onClick={() => void install()}
          >
            {highRisk ? t("dialog.updateAnyway") : t("dialog.allow")}
          </button>
        </div>
      </div>
    </ModalDialog>
  );
}
