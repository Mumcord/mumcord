import { useTranslation } from "react-i18next";
import { mouseLabel } from "../settings/pttKey";
import { spacedKey } from "./selection";

/** "Ctrl + Space", or the translated name of a mouse button. */
export function useKeyName(): (key: string) => string {
  const { t } = useTranslation("wizard");
  return (key) => {
    const mouse = mouseLabel(key);
    return mouse === null ? spacedKey(key) : t(`talk.mouse.${mouse}`);
  };
}

/** Formats an RFC 3339 date for the user's language. */
export function useDate(): (iso: string) => string {
  const { i18n } = useTranslation();
  return (iso) =>
    new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium" }).format(new Date(iso));
}
