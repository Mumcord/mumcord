import type { PermissionId } from "./catalogue";

/**
 * A decorative glyph per permission. The risk is carried by text and colour, never the glyph, so
 * the record is typed against the catalogue: a new permission id fails to compile until it has one.
 */
export const PERMISSION_ICONS: Record<PermissionId, string> = {
  "ui.panel": "▥",
  "ui.member-badge": "◉",
  "ui.profile-card": "☷",
  "ui.chat-card": "▤",
  "ui.user-menu": "⋮",
  "ui.dialog": "❏",
  "ui.webview": "▢",
  "ui.stage": "▣",
  "ui.side-panel": "◧",
  "ui.app-menu": "☰",
  "events.self": "◎",
  "events.voice": "♫",
  "events.presence": "●",
  "events.chat-activity": "✎",
  "peers.messages": "⇆",
  "peers.relay": "⇶",
  notify: "🔔",
  "random.bytes": "⚄",
  commands: "/",
  "chat.send": "➤",
  "chat.supersedes": "↻",
  "chat.triggers": "⚑",
  "chat.patterns": "⌕",
  "chat.post-image": "▧",
  "net.http": "🌐",
  "net.origins": "⇄",
  secrets: "🔑",
  "audio.effects": "♪",
  "files.save": "⤓",
  "files.import-picture": "▨",
};

/** Glyph for unknown ids, which are shown as the highest risk (see `PermissionList`). */
export const UNKNOWN_PERMISSION_ICON = "▲";
