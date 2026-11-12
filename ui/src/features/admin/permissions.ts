/**
 * Rights the admin UI needs and the server's own answers decide (Epic 4, issues #248, #254, #253
 * and #378). Admin items whose right is missing are hidden, never disabled (FR-10.2), and each
 * right lives in the channel the server answered for: Kick, Ban and Register are root-only, while
 * Mute others and Move people are per channel (FR-10.32).
 */
import type { ChannelPermissions } from "../../api";
import { getServerStateOf, type ServerState } from "../channels/serverStore";

/** The root channel, where the server-only rights sit (FR-10.1). */
const ROOT = 0;

function rootRight(
  state: ServerState,
  read: (permissions: ChannelPermissions) => boolean,
): boolean {
  const root = state.channels.get(ROOT);
  return root !== undefined && read(root.permissions);
}

/**
 * Whether the server's answers grant a right in a channel. A channel the server has not answered
 * for falls back to the root channel, because ACLs inherit downwards; a channel that answered
 * without the right has none (FR-10.1).
 */
function rightIn(
  state: ServerState,
  channelId: number,
  read: (permissions: ChannelPermissions) => boolean,
): boolean {
  const channel = state.channels.get(channelId);
  return channel === undefined ? rootRight(state, read) : read(channel.permissions);
}

/** Whether the ban list entry shows for a server: the server's own answer for the root channel. */
export function canBan(serverId: number): boolean {
  return mayBan(getServerStateOf(serverId));
}

/** Whether "Kick…" shows in a person's menu: the Kick right on the root channel (FR-10.11). */
export function canKick(serverId: number): boolean {
  return mayKick(getServerStateOf(serverId));
}

export function mayBan(state: ServerState): boolean {
  return rootRight(state, (permissions) => permissions.ban);
}

export function mayKick(state: ServerState): boolean {
  return rootRight(state, (permissions) => permissions.kick);
}

/** Register others: the root-channel right behind `/register` (FR-10.20). */
export function mayRegister(state: ServerState): boolean {
  return rootRight(state, (permissions) => permissions.register);
}

/** Mute others in one channel, which decides whether an action on a person shows (FR-10.32). */
export function mayMute(state: ServerState, channelId: number): boolean {
  return rightIn(state, channelId, (permissions) => permissions.muteDeafen);
}

/** Whether `/mute` and its siblings show at all: the right in at least one channel (FR-10.32). */
export function mayMuteAnywhere(state: ServerState): boolean {
  return [...state.channels.values()].some((channel) => channel.permissions.muteDeafen);
}

/** Move people out of one channel (FR-10.32). */
export function mayMove(state: ServerState, channelId: number): boolean {
  return rightIn(state, channelId, (permissions) => permissions.moveUsers);
}

/** Whether `/move` and `/moveall` show at all: the right in at least one channel (FR-10.32). */
export function mayMoveAnywhere(state: ServerState): boolean {
  return [...state.channels.values()].some((channel) => channel.permissions.moveUsers);
}

/**
 * Whether the Registered users tab and "Register on server…" show: the Register right on the root
 * channel (issue #256, FR-10.1, FR-10.9, FR-10.20). Mumcord hides what is not granted instead of
 * disabling it (FR-10.2).
 */
export function canRegister(serverId: number): boolean {
  return getServerStateOf(serverId).channels.get(0)?.permissions.register === true;
}

/**
 * The admin rights the server info Admin section lists (issue #258, FR-10.54), in the order of the
 * roles table (section 3). `name` is the `perm.name.*` key of the admin locale with its plain
 * name (Q3.11). Communication rights (Speak, Write messages, …) and Register self are not admin
 * rights: they unlock nothing in this epic.
 */
export const ROOT_ADMIN_RIGHTS: readonly {
  right: (permissions: ChannelPermissions) => boolean;
  name: string;
}[] = [
  { right: (p) => p.muteDeafen, name: "muteDeafen" },
  { right: (p) => p.moveUsers, name: "move" },
  { right: (p) => p.write, name: "write" },
  { right: (p) => p.makeChannel, name: "makeChannel" },
  { right: (p) => p.makeTempChannel, name: "makeTempChannel" },
  { right: (p) => p.linkChannel, name: "linkChannel" },
  { right: (p) => p.kick, name: "kick" },
  { right: (p) => p.ban, name: "ban" },
  { right: (p) => p.register, name: "register" },
  { right: (p) => p.resetUserContent, name: "resetUserContent" },
];

/** The admin rights the user holds at the root channel, as `perm.name.*` keys. */
export function rootAdminRights(state: ServerState): string[] {
  const root = state.channels.get(ROOT);
  if (root === undefined) return [];
  return ROOT_ADMIN_RIGHTS.filter(({ right }) => right(root.permissions)).map(({ name }) => name);
}

/** Whether the user has at least one admin right (the server info Admin section shows, FR-10.54). */
export function hasAdminRight(state: ServerState): boolean {
  return rootAdminRights(state).length > 0;
}
