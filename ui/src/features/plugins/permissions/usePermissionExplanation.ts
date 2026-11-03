import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { isPermissionId, permissionTextKey } from "./catalogue";

/** Mirrors MAX_APP_MENU_ITEMS in crates/plugins/src/manifest.rs; shown in the app-menu explanation. */
const MAX_APP_MENU_ITEMS = 2;

/**
 * Returns a function that gives the longer explanation of a permission for the named plugin, with
 * every placeholder of the text filled in. Unknown ids have no explanation.
 */
export function usePermissionExplanation(): (id: string, pluginName: string) => string | null {
  const { t } = useTranslation("pluginPermissions");
  return useCallback(
    (id: string, pluginName: string) =>
      isPermissionId(id)
        ? t(`explanations.${permissionTextKey(id)}`, {
            plugin: pluginName,
            count: MAX_APP_MENU_ITEMS,
          })
        : null,
    [t],
  );
}
