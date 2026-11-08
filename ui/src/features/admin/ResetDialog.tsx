/**
 * "Reset Mallory's comment?" / "Reset Mallory's avatar?" (Epic 4, issue #257, FR-10.19): asks
 * first, then clears it on the server for everyone. The person can set a new one.
 */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import { Dialog } from "../servers/Dialog";
import { initials } from "../servers/format";
import { closePersonAction, type PersonAction } from "./personActionStore";

export function ResetDialog({ action, serverName }: { action: PersonAction; serverName: string }) {
  const { t } = useTranslation("admin");
  const [failed, setFailed] = useState(false);
  const what = action.mode === "resetAvatar" ? "avatar" : "comment";

  const reset = async () => {
    try {
      await getApi().personModerate(action.serverId, action.session, {
        kind: what === "avatar" ? "resetAvatar" : "resetComment",
      });
      closePersonAction();
    } catch (error) {
      console.error("Resetting a profile failed", error);
      setFailed(true);
    }
  };

  return (
    <Dialog
      role="alertdialog"
      title={`${t(`moderate.${what}Title`, { name: action.name })} · ${serverName}`}
      titleLead={
        <span className="admin-title-mark" aria-hidden="true">
          {initials(serverName)}
        </span>
      }
      onClose={closePersonAction}
    >
      <p>{t("moderate.resetBody")}</p>
      {failed && <p role="alert">{t("moderate.failed")}</p>}
      <div className="servers-dialog__actions">
        <button type="button" className="button" onClick={closePersonAction}>
          {t("moderate.cancel")}
        </button>
        <button
          type="button"
          className="button servers-button--danger"
          onClick={() => {
            void reset();
          }}
        >
          {t("moderate.reset")}
        </button>
      </div>
    </Dialog>
  );
}
