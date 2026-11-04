/** Rights the Server administration window and the person menus need (Epic 4, issues #254, #253). */
import { getServerStateOf } from "../channels/serverStore";

/**
 * Whether the ban list entry shows for a server: the server's own answer for the root channel
 * decides it, and Mumcord hides what is not granted instead of disabling it (FR-10.1, FR-10.2).
 */
export function canBan(serverId: number): boolean {
  return getServerStateOf(serverId).channels.get(0)?.permissions.ban === true;
}

/** Whether "Kick…" shows in a person's menu: the Kick right on the root channel (FR-10.1, FR-10.11). */
export function canKick(serverId: number): boolean {
  return getServerStateOf(serverId).channels.get(0)?.permissions.kick === true;
}
