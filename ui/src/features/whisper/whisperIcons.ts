import type { IncomingWhisperKind } from "../../api";

/** Speech bubble for a whisper to people, megaphone for a shout to a channel. Decorative. */
export const WHISPER_ICON_PATHS: Record<IncomingWhisperKind, string> = {
  whisper: "M4 5h16v10H9l-5 4zM8 9h8M8 12h5",
  shout: "M3 10v4h3l7 4V6L6 10zM16 9a4 4 0 0 1 0 6M19 6a8 8 0 0 1 0 12",
};
