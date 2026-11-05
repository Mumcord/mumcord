import { useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type InstalledPlugin, type PluginUpdate } from "../../../api";
import { isPermissionId, permissionTextKey } from "../permissions/catalogue";
import { InstallProgress } from "../prompt/InstallProgress";
import { usePluginIconUrl } from "../render/assets";
import { formatSize } from "./formatSize";
import { HoldNote } from "./HoldNote";
import { needsReview, newOriginCount, newPermissionsOf } from "./reviewData";
import { ScoreChange } from "./ScoreChange";
import { UpdateReviewDialog } from "./UpdateReviewDialog";

/**
 * One waiting update (score mockup 04): icon, versions, a pill saying whether it asks for more,
 * the score before and after, the new permissions, why it waits and the button. An update that
 * needs a decision opens the review first; one that needs a newer Mumcord has no button (FR-9.18).
 */
export function UpdateRow({
  update,
  installed,
  onDone,
}: {
  update: PluginUpdate;
  installed: InstalledPlugin | undefined;
  onDone: () => Promise<void>;
}) {
  const { t, i18n } = useTranslation("pluginUpdates");
  const { t: tPerm } = useTranslation("pluginPermissions");
  const iconUrl = usePluginIconUrl(update.id, installed?.icon ?? null);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const [reviewing, setReviewing] = useState(false);
  const approval = update.approval;
  const optional = installed?.granted ?? [];

  const install = async () => {
    setBusy(true);
    setFailed(false);
    try {
      // The optional permissions that are on now stay on; new ones were never granted.
      await getApi().pluginApplyUpdate(update.id, optional);
    } catch (error: unknown) {
      console.error("Installing a plugin update failed", error);
      setFailed(true);
    } finally {
      setBusy(false);
      await onDone();
    }
  };

  const newPermissions = newPermissionsOf(update);
  const pill =
    approval?.kind === "newAuthorKey"
      ? "author"
      : newPermissions.length > 0
        ? "permission"
        : newOriginCount(update) > 0
          ? "websites"
          : "none";
  const blocked = update.hold?.kind === "blockedBySafety";
  const review = needsReview(update);

  const icon = (
    <span className="plugin-update__icon" aria-hidden="true">
      {iconUrl === null ? (
        update.name.slice(0, 1).toUpperCase()
      ) : (
        <img className="plugin-update__icon-image" src={iconUrl} alt="" />
      )}
    </span>
  );

  return (
    <li className="plugin-update">
      {icon}
      <div className="plugin-update__body">
        <div className="plugin-update__head">
          <span className="plugin-update__name">{update.name}</span>
          <span className="plugin-update__meta">
            {update.from} <span aria-hidden="true">→</span>
            <span className="plugin-update__sr-only">{t("versionsTo")}</span>{" "}
            <strong className="plugin-update__to">{update.to}</strong> ·{" "}
            {formatSize(update.size, i18n.language)}
          </span>
          <span className={`plugin-update__pill plugin-update__pill--${pill}`}>
            {t(`pill.${pill}`)}
          </span>
        </div>
        {update.review.risk !== null && <ScoreChange risk={update.review.risk} />}
        {approval?.kind === "newAuthorKey" && (
          <p role="alert" className="plugin-update__note plugin-update__note--warn">
            {t("newAuthorKey")}
          </p>
        )}
        {update.failed !== null && (
          <p role="status" className="plugin-update__note plugin-update__note--warn">
            {t("rolledBack", { version: update.to })} {update.failed}
          </p>
        )}
        {newPermissions.length > 0 && (
          <ul className="plugin-update__permissions" aria-label={t("newPermissions")}>
            {newPermissions.map((id) => (
              <li key={id}>
                <span className="plugin-update__permission-mark" aria-hidden="true">
                  +
                </span>{" "}
                {isPermissionId(id) ? tPerm(`texts.${permissionTextKey(id)}`) : id}
              </li>
            ))}
          </ul>
        )}
        {update.needsMumcord !== null ? (
          <p className="plugin-update__hold">{t("needsMumcord")}</p>
        ) : (
          update.hold !== null && <HoldNote update={update} hold={update.hold} />
        )}
        <InstallProgress id={update.id} name={update.name} />
        {failed && (
          <p role="alert" className="settings-error">
            {t("installFailed", { name: update.name })}
          </p>
        )}
      </div>
      {update.needsMumcord === null && !blocked && (
        <div className="plugin-update__actions">
          <button
            type="button"
            className="settings-button settings-button--primary"
            disabled={busy}
            onClick={() => {
              if (review) setReviewing(true);
              else void install();
            }}
          >
            {review ? t("review") : t("update")}{" "}
            <span className="plugin-update__sr-only">{update.name}</span>
          </button>
        </div>
      )}
      {reviewing && (
        <UpdateReviewDialog
          update={update}
          icon={icon}
          optional={optional}
          onClose={() => {
            setReviewing(false);
          }}
          onDone={() => {
            setReviewing(false);
            void onDone();
          }}
        />
      )}
    </li>
  );
}
