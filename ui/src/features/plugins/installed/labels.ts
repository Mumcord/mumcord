import type { InstalledPlugin } from "../../../api";
import { permissionTextKey } from "../permissions/catalogue";

/** A label on a plugin row; `key` is in the `pluginInstalled` namespace. */
export interface RowLabel {
  key: string;
  values?: Record<string, string>;
  /** `warning`: the plugin is off or something needs the user's attention. */
  tone: "neutral" | "warning";
}

/** Where the plugin came from: Official, Unofficial · <repository> or Developer plugin (FR-6.61). */
export function sourceLabel(plugin: InstalledPlugin): RowLabel {
  switch (plugin.source.kind) {
    case "official":
      return { key: "labels.official", tone: "neutral" };
    case "repo":
      return plugin.source.name === ""
        ? { key: "labels.unofficialUnknown", tone: "warning" }
        : { key: "labels.unofficial", values: { repository: plugin.source.name }, tone: "warning" };
    case "dev":
      return { key: "labels.developer", tone: "neutral" };
  }
}

/**
 * Why the plugin is not running, or a warning about it; `null` when there is nothing to say. The
 * "Turn off all" and `--no-plugins` states have their own banner, so their rows stay quiet.
 */
export function stateLabel(plugin: InstalledPlugin): RowLabel | null {
  if (plugin.disabledReason === "revoked") {
    return {
      key: "labels.revoked",
      values: { reason: plugin.revocation ?? "" },
      tone: "warning",
    };
  }
  if (plugin.disabledReason === "tampered") return { key: "labels.tampered", tone: "warning" };
  switch (plugin.status.kind) {
    case "autoOff":
      return { key: "labels.autoOff", tone: "warning" };
    case "backingOff":
      return { key: "labels.backingOff", tone: "warning" };
    case "notLoaded":
      return { key: "labels.notLoaded", tone: "warning" };
    default:
      break;
  }
  if (plugin.disabledReason === "permission") return { key: "labels.permission", tone: "warning" };
  // A revocation entry with severity "warn" leaves the plugin on and only shows its reason.
  if (plugin.revocation !== null) {
    return {
      key: "labels.revocationWarning",
      values: { reason: plugin.revocation },
      tone: "warning",
    };
  }
  return null;
}

/** The name to show: a plugin that could not be loaded has none, so fall back to its id. */
export function displayName(plugin: InstalledPlugin): string {
  return plugin.name === "" ? plugin.id : plugin.name;
}

/** Up to two letters for the row icon (plugins have no icon of their own here). */
export function initials(name: string): string {
  const words = name.split(/[\s_-]+/).filter((word) => word !== "");
  const letters =
    words.length >= 2 ? `${words[0]?.[0] ?? ""}${words[1]?.[0] ?? ""}` : name.slice(0, 2);
  return letters.toUpperCase();
}

/** The plugin may draw a badge next to names (FR-6.63). */
export const BADGE_PERMISSION = "ui.member-badge";
/** The plugin talks to other Mumcord users, so it has a visibility choice. */
export const MESSAGES_PERMISSION = "peers.messages";

export function requests(plugin: InstalledPlugin, permission: string): boolean {
  return plugin.required.includes(permission) || plugin.optional.some((p) => p.id === permission);
}

/** Chip order on a card: what the plugin shows, then what it reads or does. */
const CHIP_PERMISSIONS = [
  "ui.panel",
  "ui.member-badge",
  "ui.profile-card",
  "ui.chat-card",
  "ui.user-menu",
  "ui.dialog",
  "ui.webview",
  "events.self",
  "events.voice",
  "events.presence",
  "events.chat-activity",
  "notify",
  "random.bytes",
  "commands",
  "chat.send",
  "chat.supersedes",
  "chat.triggers",
  "chat.patterns",
  "net.http",
  "secrets",
] as const;

/**
 * Keys (under `chips` in `pluginInstalled`) for the permissions the plugin asks for and has: all
 * required ones and the optional ones that are on. `peers.messages` is shown as the visibility
 * chip instead.
 */
export function permissionChips(plugin: InstalledPlugin): string[] {
  const active = new Set([...plugin.required, ...plugin.granted]);
  return CHIP_PERMISSIONS.filter((id) => active.has(id)).map(permissionTextKey);
}
