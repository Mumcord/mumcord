export type PermissionRisk = "green" | "yellow" | "red";

/**
 * Risk of each permission id. Mirrors `crates/plugins/src/permissions.rs` (the closed catalogue,
 * FR-6.27); the Rust side stays the source of truth for what a manifest may ask for.
 */
export const PERMISSION_RISK = {
  "ui.panel": "green",
  "ui.member-badge": "green",
  "ui.profile-card": "green",
  "ui.chat-card": "green",
  "ui.user-menu": "green",
  "ui.dialog": "green",
  "events.self": "green",
  "events.voice": "yellow",
  "events.presence": "yellow",
  "events.chat-activity": "yellow",
  "peers.messages": "yellow",
  notify: "green",
  "random.bytes": "green",
  commands: "green",
  "chat.send": "yellow",
  "chat.supersedes": "green",
  "chat.triggers": "yellow",
  "net.http": "red",
  secrets: "red",
  "ui.webview": "yellow",
} as const satisfies Record<string, PermissionRisk>;

export type PermissionId = keyof typeof PERMISSION_RISK;

export function isPermissionId(id: string): id is PermissionId {
  return Object.hasOwn(PERMISSION_RISK, id);
}

/** Translation key of a permission's plain-language text; ids contain dots, which i18next treats as nesting. */
export function permissionTextKey(id: PermissionId): string {
  return id.replace(/[.-](\w)/g, (_, c: string) => c.toUpperCase());
}

/** One permission a plugin asks for. `suggested` optional permissions start switched on (FR-6.29). */
export interface RequestedPermission {
  id: string;
  required: boolean;
  suggested?: boolean;
}

/** The optional permissions that start switched on: only the suggested ones. */
export function defaultOptionalSelection(requested: readonly RequestedPermission[]): Set<string> {
  return new Set(requested.filter((p) => !p.required && p.suggested === true).map((p) => p.id));
}
