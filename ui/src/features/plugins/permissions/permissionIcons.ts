import type { PermissionId } from "./catalogue";

/**
 * A drawn icon per permission (issue #588): an SVG path on a 24×24 grid, drawn with the settings
 * icons' stroke so every system shows the same shape (text glyphs came out in each system's own
 * font and size). The risk is carried by text and colour, never the icon, so the record is typed
 * against the catalogue: a new permission id fails to compile until it has one.
 */
export const PERMISSION_ICONS: Record<PermissionId, string> = {
  // Clock.
  activity: "M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20zM12 6v6l4 2",
  // Window with a panel on the right.
  "ui.panel": "M3 3h18v18H3zM15 3v18",
  // Ring with a dot: a badge next to a name.
  "ui.member-badge": "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z",
  // ID card.
  "ui.profile-card":
    "M3 5h18v14H3zM8 11a2 2 0 1 0 0-4 2 2 0 0 0 0 4zM5 16a3 3 0 0 1 6 0M14 9h4M14 13h4",
  // Speech bubble with lines.
  "ui.chat-card": "M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2zM7 8h10M7 12h6",
  // Three dots, vertical.
  "ui.user-menu":
    "M12 4a1 1 0 1 0 0 2a1 1 0 1 0 0-2zM12 11a1 1 0 1 0 0 2a1 1 0 1 0 0-2zM12 18a1 1 0 1 0 0 2a1 1 0 1 0 0-2z",
  // Window with a title bar.
  "ui.dialog": "M3 4h18v16H3zM3 9h18",
  // Window with code brackets.
  "ui.webview": "M3 4h18v16H3zM9 10l-2 2 2 2M15 10l2 2-2 2",
  // Screen on a stand.
  "ui.stage": "M2 4h20v13H2zM8 21h8M12 17v4",
  // Window with a panel on the left.
  "ui.side-panel": "M3 3h18v18H3zM9 3v18",
  // Four tiles.
  "ui.app-menu": "M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h6v6h-6z",
  // One person.
  "events.self": "M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2M12 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8z",
  // Microphone.
  "events.voice":
    "M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3zM19 10v2a7 7 0 0 1-14 0v-2M12 19v3",
  // Two people.
  "events.presence":
    "M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2M9 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM23 21v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75",
  // Pen.
  "events.chat-activity": "M12 20h9M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4z",
  // Arrows both ways.
  "peers.messages": "M7 7h14M17 3l4 4-4 4M17 17H3M7 13l-4 4 4 4",
  // Three connected nodes.
  "peers.relay":
    "M18 8a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM6 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM18 22a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM8.6 13.5l6.8 4M15.4 6.5l-6.8 4",
  // Bell.
  notify: "M18 8a6 6 0 0 0-12 0c0 7-3 9-3 9h18s-3-2-3-9M13.7 21a2 2 0 0 1-3.4 0",
  // Die.
  "random.bytes":
    "M4 4h16v16H4zM8.5 7.5a1 1 0 1 0 0 2a1 1 0 1 0 0-2zM15.5 14.5a1 1 0 1 0 0 2a1 1 0 1 0 0-2zM15.5 7.5a1 1 0 1 0 0 2a1 1 0 1 0 0-2zM8.5 14.5a1 1 0 1 0 0 2a1 1 0 1 0 0-2zM12 11a1 1 0 1 0 0 2a1 1 0 1 0 0-2z",
  // Prompt.
  commands: "M4 17l6-6-6-6M12 19h8",
  // Paper plane.
  "chat.send": "M22 2L11 13M22 2l-7 20-4-9-9-4z",
  // Circular arrow.
  "chat.supersedes": "M23 4v6h-6M20.5 15a9 9 0 1 1-2.1-9.4L23 10",
  // Flag.
  "chat.triggers": "M4 15s1-1 4-1 5 2 8 2 4-1 4-1V3s-1 1-4 1-5-2-8-2-4 1-4 1zM4 22v-7",
  // Magnifier.
  "chat.patterns": "M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16zM21 21l-4.35-4.35",
  // Picture.
  "chat.post-image": "M3 3h18v18H3zM8.5 10a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3zM21 15l-5-5L5 21",
  // Globe.
  "net.http":
    "M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20zM2 12h20M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z",
  // Chain links.
  "net.origins":
    "M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71",
  // Key.
  secrets:
    "M21 2l-2 2m-7.6 7.6a5.5 5.5 0 1 1-7.78 7.78 5.5 5.5 0 0 1 7.78-7.78zm0 0L15.5 7.5m0 0l3 3L22 7l-3-3m-3.5 3.5L19 4",
  // Speaker with sound waves.
  "audio.effects":
    "M11 5L6 9H2v6h4l5 4V5zM19.07 4.93a10 10 0 0 1 0 14.14M15.54 8.46a5 5 0 0 1 0 7.07",
  // Download into a tray.
  "files.save": "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M7 10l5 5 5-5M12 15V3",
  // Picture with a plus.
  "files.import-picture":
    "M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h7M16 5h6M19 2v6M21 15l-5-5L5 21",
};

/** Warning triangle for unknown ids, which are shown as the highest risk (see `PermissionList`). */
export const UNKNOWN_PERMISSION_ICON =
  "M10.3 3.9L1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0zM12 9v4M12 17h.01";
