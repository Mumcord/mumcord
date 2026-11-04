/**
 * Kick dialog (issue #253, FR-10.57): the question as the title, the person line, the line saying
 * they can reconnect right away, the optional reason that is shown to them, and Cancel / Kick.
 * Enter confirms and Escape cancels (FR-10.56); a refusal stays inside the open dialog (FR-10.59).
 * Kicking yourself is never offered (issue #253).
 */
import { useId } from "react";
import { useTranslation } from "react-i18next";
import { Dialog } from "../servers/Dialog";
import { initials } from "../servers/format";
import { actionErrorText } from "./errors";
import { PersonLine } from "./PersonLine";
import { closePersonAction, setDraft, sendKick, type PersonAction } from "./personActionStore";

export function KickDialog({ action, serverName }: { action: PersonAction; serverName: string }) {
  const { t } = useTranslation(["admin", "common"]);
  const reasonId = useId();
  const noteId = useId();

  const reason = action.draft.reason.trim() === "" ? null : action.draft.reason.trim();

  return (
    <Dialog
      title={`${t("kickTitle", { name: action.name })} · ${serverName}`}
      titleLead={
        <span className="admin-title-mark" aria-hidden="true">
          {initials(serverName)}
        </span>
      }
      onClose={() => {
        closePersonAction();
      }}
      describedBy={noteId}
      className="admin-dialog"
    >
      <PersonLine action={action} />
      <p id={noteId} className="admin-dialog__note">
        {t("canReconnect")}
      </p>

      <form
        className="admin-form"
        onSubmit={(event) => {
          event.preventDefault();
          void sendKick(reason);
        }}
      >
        <div className="admin-form__field">
          <label htmlFor={reasonId}>{t("reason")}</label>
          <input
            id={reasonId}
            type="text"
            value={action.draft.reason}
            onChange={(event) => {
              setDraft({ reason: event.target.value });
            }}
          />
          <span className="admin-form__help">{t("reasonShownToThem", { name: action.name })}</span>
        </div>

        {action.error ? (
          <p className="admin-error" role="alert">
            {actionErrorText(t, action.error)}
          </p>
        ) : null}

        <div className="servers-dialog__actions">
          <button
            type="button"
            className="button"
            onClick={() => {
              closePersonAction();
            }}
          >
            {t("common:cancel")}
          </button>
          <button type="submit" className="button servers-button--danger" disabled={action.busy}>
            {t("kick")}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
