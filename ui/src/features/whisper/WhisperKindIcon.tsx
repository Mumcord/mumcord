import type { IncomingWhisperKind } from "../../api";
import { WHISPER_ICON_PATHS } from "./whisperIcons";

/** Speech bubble for a whisper, megaphone for a shout. Decorative. */
export function WhisperKindIcon({ kind }: { kind: IncomingWhisperKind }) {
  return (
    <svg className="whisper-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
      <path
        d={WHISPER_ICON_PATHS[kind]}
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
