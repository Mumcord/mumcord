import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type InstalledPlugin, type PluginUpdate } from "../../../api";
import { PermissionList } from "../permissions/PermissionList";
import { formatSize } from "./formatSize";

const NO_OPTIONAL: ReadonlySet<string> = new Set();

/** One waiting update: what changes, why it needs approval, and the button that installs it. */
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
  const detailsId = useId();
  const [review, setReview] = useState(false);
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

  return (
    <li className="plugin-update">
      <div className="plugin-update__head">
        <span className="plugin-update__name">{update.name}</span>
        <span className="plugin-update__meta">
          {t("versions", { from: update.from, to: update.to })} ·{" "}
          {formatSize(update.size, i18n.language)}
        </span>
      </div>
      {approval?.kind === "newPermissions" && (
        <p className="plugin-update__note">{t("needsPermissions")}</p>
      )}
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
      {review && approval?.kind === "newPermissions" && (
        <div id={detailsId} className="plugin-update__review">
          <PermissionList
            requested={approval.permissions.map((id) => ({ id, required: true }))}
            enabled={NO_OPTIONAL}
            onToggle={() => undefined}
            readOnly
          />
        </div>
      )}
      {failed && (
        <p role="alert" className="settings-error">
          {t("installFailed", { name: update.name })}
        </p>
      )}
      <div className="plugin-update__actions">
        {approval !== null && !review ? (
          <button
            type="button"
            className="settings-button"
            aria-expanded={false}
            onClick={() => {
              setReview(true);
            }}
          >
            {t("review")}
          </button>
        ) : (
          <button
            type="button"
            className="settings-button"
            disabled={busy}
            onClick={() => void install()}
          >
            {approval !== null ? t("approve") : t("update")}
          </button>
        )}
      </div>
    </li>
  );
}
