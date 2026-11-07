/**
 * The saved activity history (#594): each server's feed is kept in local storage so it is still
 * there after a reconnect or a restart. Only on this device: nothing is synced or sent, and only
 * what the feed shows is stored (names, channel names, plugin texts), never chat contents.
 *
 * Failures never break the feed: an unreadable entry loads as an empty history, and a failed write
 * is logged and leaves the in-memory feed as it is.
 */
import type { ActivityEvent } from "./activityStore";

const KEY_PREFIX = "mumcord.activity.";

const KINDS: ReadonlySet<string> = new Set<ActivityEvent["kind"]>([
  "connected",
  "disconnected",
  "moved",
  "screenShare",
  "watchParty",
  "pluginActivity",
  "pluginEvent",
]);

const keyOf = (serverId: number): string => `${KEY_PREFIX}${String(serverId)}`;

/**
 * Only the outer shape is checked: the entries were written by this client, so a full schema check
 * would guard against nothing but a hand-edited storage. A wrong outer shape (older or corrupt data)
 * is dropped instead of rendered.
 */
function isEvent(value: unknown, serverId: number): value is ActivityEvent {
  if (typeof value !== "object" || value === null) return false;
  const entry = value as Record<string, unknown>;
  return (
    typeof entry.kind === "string" &&
    KINDS.has(entry.kind) &&
    typeof entry.at === "number" &&
    entry.serverId === serverId
  );
}

/** The saved events of one server, newest first, all marked as from before this session. */
export function loadHistory(serverId: number): ActivityEvent[] {
  let raw: string | null;
  try {
    raw = localStorage.getItem(keyOf(serverId));
  } catch {
    return [];
  }
  if (raw === null) return [];
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (error: unknown) {
    console.error("The saved activity history is unreadable", error);
    return [];
  }
  if (!Array.isArray(parsed)) return [];
  return parsed
    .filter((entry): entry is ActivityEvent => isEvent(entry, serverId))
    .map((event) => ({ ...event, earlier: true }));
}

export function saveHistory(serverId: number, events: readonly ActivityEvent[]): void {
  try {
    if (events.length === 0) localStorage.removeItem(keyOf(serverId));
    else localStorage.setItem(keyOf(serverId), JSON.stringify(events));
  } catch (error: unknown) {
    console.error("Saving the activity history failed", error);
  }
}

/** Removes the saved history of every server. */
export function clearSavedHistory(): void {
  try {
    const keys: string[] = [];
    for (let index = 0; index < localStorage.length; index += 1) {
      const key = localStorage.key(index);
      if (key?.startsWith(KEY_PREFIX)) keys.push(key);
    }
    for (const key of keys) localStorage.removeItem(key);
  } catch (error: unknown) {
    console.error("Clearing the activity history failed", error);
  }
}
