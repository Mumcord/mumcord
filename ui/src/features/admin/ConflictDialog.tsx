/** The conflict a save found: the entry changed on the server meanwhile (FR-10.66, FR-10.43). */
import { useTranslation } from "react-i18next";
import { Dialog } from "../servers/Dialog";
import { clearConflict, load, saveChanges } from "./banStore";

export function ConflictDialog() {
  const { t } = useTranslation(["admin", "common"]);
  return (
    <Dialog title={t("admin:conflictTitle")} onClose={clearConflict} className="admin-dialog">
      <p>{t("admin:conflictLine")}</p>
      <div className="servers-dialog__actions">
        <button type="button" className="button" onClick={clearConflict}>
          {t("common:back")}
        </button>
        <button type="button" className="button" onClick={() => void load()}>
          {t("admin:conflictReload")}
        </button>
        <button
          type="button"
          className="button button--primary"
          onClick={() => void saveChanges(true)}
        >
          {t("admin:conflictSaveAnyway")}
        </button>
      </div>
    </Dialog>
  );
}
