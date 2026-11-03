import { useTranslation } from "react-i18next";
import { endsWhen } from "./fileFormat";

/** "ends tomorrow at 20:04". */
export function useEndsText(expires: number): string {
  const { t, i18n } = useTranslation("files");
  const { key, time } = endsWhen(expires, new Date(), i18n.language);
  return t(`when.${key}`, { time });
}
