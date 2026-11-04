/**
 * The person line both dialogs show: who, which channel, and how long they are connected
 * (issue #253, FR-10.57, mockup `03-ban-dialog`).
 */
import { useTranslation } from "react-i18next";
import { formatDuration } from "../files/cards/fileFormat";
import type { PersonAction } from "./personActionStore";

export function PersonLine({ action }: { action: PersonAction }) {
  const { t, i18n } = useTranslation("admin");
  const parts = [t("personLine", { name: action.name, channel: action.channel })];
  const online = action.target?.onlineSecs;
  if (online !== null && online !== undefined) {
    parts.push(t("connectedFor", { time: formatDuration(online, i18n.language) }));
  }
  return <p className="admin-dialog__person">{parts.join(" · ")}</p>;
}
