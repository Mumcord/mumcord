import { useEffect } from "react";
import { getApi } from "../../api";
import { whatsNewOnStart } from "./changelog";

/**
 * Opens "What's new" once after an update (issue #145). Mounted with the app, before the
 * wizard, so a first start is told apart from an update by the settings as they were stored.
 */
export function WhatsNewOnStart() {
  useEffect(() => {
    const api = getApi();
    Promise.all([api.settingsGet(), api.appInfo()])
      .then(async ([settings, info]) => {
        const { open, remember } = whatsNewOnStart({
          version: info.version,
          wizardCompleted: settings.wizardCompleted,
          whatsNewAuto: settings.whatsNewAuto,
          whatsNewSeen: settings.whatsNewSeen,
        });
        if (remember) await api.whatsNewMarkSeen();
        if (open) await api.changelogOpen();
      })
      .catch((error: unknown) => {
        console.error("Showing what's new after the update failed", error);
      });
  }, []);

  return null;
}
