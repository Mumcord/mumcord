import type { ChannelInfo, SavedChannelPassword, ServerInfo, User } from "../../api";
import { htmlToText } from "../chat/sanitize";

/** Channel names from below the root down to `channelId`; empty for the root and unknown ids. */
export function channelPath(
  channels: ReadonlyMap<number, ChannelInfo>,
  channelId: number,
): string[] {
  const path: string[] = [];
  const seen = new Set<number>();
  let channel = channels.get(channelId);
  // `seen` guards against a cycle in what the server sent.
  while (channel && channel.parentId !== null && !seen.has(channel.id)) {
    seen.add(channel.id);
    path.unshift(channel.name);
    channel = channels.get(channel.parentId);
  }
  return path;
}

/**
 * `mumble://` link to a channel in the form official clients understand (host, port, channel
 * path, protocol version). Never contains a user name or password. `null` without an address.
 */
export function channelLink(info: ServerInfo, path: string[]): string | null {
  const address = info.serverAddress;
  if (!address) return null;
  const host = address.includes(":") && !address.startsWith("[") ? `[${address}]` : address;
  const port = info.serverPort === null ? "" : `:${String(info.serverPort)}`;
  const channel = path.map((name) => encodeURIComponent(name)).join("/");
  return `mumble://${host}${port}/${channel}?version=1.2.0`;
}

/** The description as one plain line for the header (the HTML is untrusted). */
export function shortDescription(html: string | null): string {
  if (html === null) return "";
  // Line breaks and block ends separate words; the text alone would glue them together.
  return htmlToText(html.replace(/<br\b[^>]*>|<\/(?:p|div|li|h\d)>/gi, "$& "));
}

/** People currently in the channel. */
export function userCount(users: ReadonlyMap<number, User>, channelId: number): number {
  let count = 0;
  for (const user of users.values()) if (user.channelId === channelId) count += 1;
  return count;
}

/** True when a password saved for this server was entered for this channel (#146). */
export function hasSavedPassword(saved: SavedChannelPassword[], channelId: number): boolean {
  return saved.some((password) =>
    password.channels.some((c) => c.channelId === channelId && c.found !== false),
  );
}

export type Permission = "enter" | "speak" | "textMessage" | "whisper" | "listen" | "write";

/** The own permissions shown in channel info, in a fixed order, each allowed or not. */
export function permissionList(
  permissions: ChannelInfo["permissions"],
): { permission: Permission; allowed: boolean }[] {
  const order: Permission[] = ["enter", "speak", "textMessage", "whisper", "listen", "write"];
  return order.map((permission) => ({ permission, allowed: permissions[permission] }));
}

/**
 * People who can be moved into `channelId`: everyone else on the server who is in another channel
 * the own user may move people out of. Sorted by their channel, then by name.
 */
export function movablePeople(
  users: ReadonlyMap<number, User>,
  channels: ReadonlyMap<number, ChannelInfo>,
  selfSession: number | null,
  channelId: number,
): User[] {
  return [...users.values()]
    .filter(
      (user) =>
        user.session !== selfSession &&
        user.channelId !== channelId &&
        channels.get(user.channelId)?.permissions.moveUsers === true,
    )
    .sort(
      (a, b) =>
        (channels.get(a.channelId)?.name ?? "").localeCompare(
          channels.get(b.channelId)?.name ?? "",
        ) || a.name.localeCompare(b.name),
    );
}
