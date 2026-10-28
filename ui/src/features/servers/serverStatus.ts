/** State of one server-bar icon and its text label (spec §5.2). */
import type { TFunction } from "i18next";
import type { ServersState } from "./store";

/**
 * "error": the connection was lost and reconnecting gave up or was cancelled; the menu offers
 * "Reconnect". "failed": the last connect attempt failed; it outlives the status bar message,
 * which hides after a while.
 */
export type ServerStatus = "connected" | "connecting" | "reconnecting" | "error" | "failed" | null;

/** Status of one icon (spec §5.2); `null` is a saved server that is not connected. */
export function statusOf(state: ServersState, id: number): ServerStatus {
  const connection = state.connections.get(id);
  if (connection && connection.state !== "disconnected") return connection.state;
  const { ended } = state;
  if (ended?.serverId !== id) return null;
  if (ended.afterLoss) return "error";
  return ended.error ? "failed" : null;
}

export interface ServerBadges {
  status: ServerStatus;
  voice: boolean;
  unread: boolean;
  mentions: number;
}

/**
 * Screen-reader name and tooltip of an icon: every state as text (spec §5.2), e.g.
 * "Raid (connected), voice server, 3 mentions". A saved, not connected server is just its name.
 */
export function serverLabel(t: TFunction<"servers">, name: string, badges: ServerBadges): string {
  if (badges.status === null) return name;
  const details = [
    t("bar.label", { server: name, state: t(`bar.state.${badges.status}`) }),
    ...(badges.voice ? [t("bar.state.voice")] : []),
    ...(badges.mentions > 0 ? [t("bar.state.mentions", { mentions: badges.mentions })] : []),
    ...(badges.unread && badges.mentions === 0 ? [t("bar.state.unread")] : []),
  ];
  return details.join(", ");
}
