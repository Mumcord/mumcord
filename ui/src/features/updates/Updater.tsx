import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { getApi } from "../../api";
import { useServers as useConnectedServers } from "../channels";
import { openInBrowser } from "../chat/openLink";
import { onUpdateDialogRequest } from "./dialogRequests";
import { UpdateDialog, type UpdateActions } from "./UpdateDialog";
import { UpdateToast } from "./UpdateToast";
import { useUpdateStatus } from "./useUpdateStatus";

const engineActions: UpdateActions = {
  download: async () => {
    await getApi().updateDownload();
  },
  cancel: async () => {
    await getApi().updateCancel();
  },
  restart: async () => {
    await getApi().updateRestart();
  },
  skip: async (version) => {
    const api = getApi();
    const settings = await api.settingsGet();
    await api.settingsUpdate({ ...settings, updateSkipped: version });
  },
  openReleasePage: openInBrowser,
};

/**
 * The in-app updater's main-window UI (FR-3.145): the update dialog when the engine asks for it
 * or the user opens it, and a toast while an installed update waits for a restart.
 */
export function Updater({ actions = engineActions }: { actions?: UpdateActions }) {
  const { t } = useTranslation("appwindow");
  const [dialogOpen, setDialogOpen] = useState(false);
  const [toastHidden, setToastHidden] = useState(false);
  const showDialog = useCallback(() => {
    setDialogOpen(true);
    // Something new happened: the toast may come back after this dialog closes.
    setToastHidden(false);
  }, []);
  const { status, secondsLeft } = useUpdateStatus(showDialog);
  const connectedServers = useConnectedServers().size;
  useEffect(() => onUpdateDialogRequest(showDialog), [showDialog]);

  const readyVersion = status?.phase === "ready" ? (status.offer?.version ?? null) : null;
  useEffect(() => {
    if (readyVersion !== null) announce(t("updater.ready.announce", { version: readyVersion }));
  }, [readyVersion, t]);

  if (status?.offer == null) return null;
  if (dialogOpen) {
    return (
      <UpdateDialog
        status={status}
        secondsLeft={secondsLeft}
        connectedServers={connectedServers}
        actions={actions}
        onClose={() => {
          setDialogOpen(false);
        }}
      />
    );
  }
  if (readyVersion === null || toastHidden) return null;
  return (
    <UpdateToast
      version={readyVersion}
      connectedServers={connectedServers}
      onRestart={() => {
        actions.restart().catch((error: unknown) => {
          console.error("Restarting for the update failed", error);
          announce(t("updater.actionFailed"), "assertive");
        });
      }}
      onShowDialog={showDialog}
      onLater={() => {
        setToastHidden(true);
      }}
    />
  );
}
