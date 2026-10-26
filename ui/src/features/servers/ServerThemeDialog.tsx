import { useState, type SubmitEvent } from "react";
import { useTranslation } from "react-i18next";
import type { SavedServer } from "../../api";
import { Dialog } from "./Dialog";
import { displayName } from "./format";
import { ServerThemeSelect } from "./ServerThemeSelect";
import { saveServerTheme, toApiError } from "./store";

interface ServerThemeDialogProps {
  server: SavedServer;
  onClose: () => void;
}

/** "Theme…" from the server's context menu (FR-2.17). */
export function ServerThemeDialog({ server, onClose }: ServerThemeDialogProps) {
  const { t } = useTranslation(["servers", "common"]);
  const [themeId, setThemeId] = useState(server.themeId);
  const [failed, setFailed] = useState(false);
  const [busy, setBusy] = useState(false);

  const onSubmit = async (event: SubmitEvent) => {
    event.preventDefault();
    setBusy(true);
    setFailed(false);
    try {
      await saveServerTheme(server.id, themeId);
      onClose();
    } catch (error) {
      toApiError(error);
      setFailed(true);
      setBusy(false);
    }
  };

  return (
    <Dialog title={t("theme.title", { server: displayName(server) })} onClose={onClose}>
      <form className="servers-form" onSubmit={(event) => void onSubmit(event)} noValidate>
        <div className="servers-form__field">
          <ServerThemeSelect label={t("theme.label")} value={themeId} onChange={setThemeId} />
          <p className="servers-form__hint">{t("theme.hint")}</p>
        </div>
        {failed && (
          <p className="servers-form__error" role="alert">
            {t("theme.failed")}
          </p>
        )}
        <div className="servers-dialog__actions">
          <button type="button" className="button" onClick={onClose}>
            {t("common:cancel")}
          </button>
          <button type="submit" className="button button--primary" disabled={busy}>
            {t("common:save")}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
