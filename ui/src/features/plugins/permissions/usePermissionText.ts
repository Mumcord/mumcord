import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { isPermissionId, permissionTextKey } from "./catalogue";

/**
 * Returns a function that turns a permission id into its plain-language text. Unknown ids cannot
 * come from a validated manifest; they are shown as the raw id rather than hidden.
 */
export function usePermissionText(): (id: string) => string {
  const { t } = useTranslation("pluginPermissions");
  return useCallback(
    (id: string) => (isPermissionId(id) ? t(`texts.${permissionTextKey(id)}`) : id),
    [t],
  );
}
