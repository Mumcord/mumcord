import { useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type InstalledPlugin, type PluginUpdate } from "../../../api";
import { isPermissionId, permissionTextKey } from "../permissions/catalogue";
import { usePluginIconUrl } from "../render/assets";
import { formatSize } from "./formatSize";

/**
 * One waiting update: icon, versions, a pill saying whether it asks for more permissions, the new
 * permissions themselves and the button that installs it. The permissions are on show before the
 * button is pressed, so "Review and update" is the review.
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
  const approval = update.approval;

  const install = async () => {
    setBusy(true);
    setFailed(false);
    try {
      // The optional permissions that are on now stay on; new ones were never granted.
      await getApi().pluginApplyUpdate(update.id, installed?.granted ?? []);
    } catch (error: unknown) {
      console.error("Installing a plugin update failed", error);
      setFailed(true);
    } finally {
      setBusy(false);
      await onDone();
    }
  };

  const newPermissions = approval?.kind === "newPermissions" ? approval.permissions : [];
  const pill =
    approval === null ? "none" : approval.kind === "newPermissions" ? "permission" : "author";

  return (
    <li className="plugin-update">
      <span className="plugin-update__icon" aria-hidden="true">
        {iconUrl === null ? (
          update.name.slice(0, 1).toUpperCase()
        ) : (
          <img className="plugin-update__icon-image" src={iconUrl} alt="" />
        )}
      </span>
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
        {failed && (
          <p role="alert" className="settings-error">
            {t("installFailed", { name: update.name })}
          </p>
        )}
      </div>
      <div className="plugin-update__actions">
        <button
          type="button"
          className="settings-button settings-button--primary"
          disabled={busy}
          onClick={() => void install()}
        >
          {approval !== null ? t("review") : t("update")}
        </button>
      </div>
    </li>
  );
}
