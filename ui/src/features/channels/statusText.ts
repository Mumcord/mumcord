/** Status of a person as text, shared by the channel tree and the member list. */
import { useTranslation } from "react-i18next";
import type { User } from "../../api";

export function isMuted(user: User): boolean {
  return user.selfMute || user.serverMute;
}

export function isDeafened(user: User): boolean {
  return user.selfDeaf || user.serverDeaf;
}

/** Spoken status parts, e.g. ["talking", "muted"]. */
export function useStatusText() {
  const { t } = useTranslation("channels");
  return (user: User, talking: boolean, listener = false): string[] => {
    const parts: string[] = [];
    if (listener) parts.push(t("status.listening"));
    if (talking) parts.push(t("status.talking"));
    if (isDeafened(user)) parts.push(t("status.deafened"));
    else if (isMuted(user)) parts.push(t("status.muted"));
    if (user.localMute) parts.push(t("status.localMuted"));
    if (!user.registered) parts.push(t("status.unregistered"));
    return parts;
  };
}
