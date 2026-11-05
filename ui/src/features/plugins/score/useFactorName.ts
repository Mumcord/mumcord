import { useTranslation } from "react-i18next";
import { usePermissionText } from "../permissions/usePermissionText";
import type { FactorName } from "./breakdownRows";

/** The text of a factor's name, shared with the biggest-reasons list. */
export function useFactorName(): (name: FactorName) => string {
  const { t } = useTranslation("pluginScore");
  const permissionText = usePermissionText();
  return (name) =>
    name.kind === "permission"
      ? permissionText(name.id)
      : name.kind === "text"
        ? t(name.key)
        : name.text;
}
