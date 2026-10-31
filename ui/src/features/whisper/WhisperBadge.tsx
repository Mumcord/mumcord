import { useT } from "../../i18n/useT";
import { useActiveWhisperTarget, useIncomingWhisper, whisperKind } from "./whisperStore";
import "./whisper.css";

/**
 * "whispers" / "shouts" next to a person who whispers or shouts to the user right now (issue #67,
 * mockup `67-whisper-live`), and "whispering" / "shouting" next to the user while they do. Used in
 * the channel tree and the member list, like the "Live" marker.
 */
export function WhisperBadge({ session, self }: { session: number; self: boolean }) {
  const t = useT("whisper");
  const incoming = useIncomingWhisper(session);
  const own = useActiveWhisperTarget();
  const kind = self ? (own === null ? null : whisperKind(own.receivers)) : incoming;
  if (kind === null) return null;
  const word = self
    ? t(kind === "whisper" ? "badge.ownWhisper" : "badge.ownShout")
    : t(`badge.${kind}`);
  const label = self
    ? t(kind === "whisper" ? "badge.ownWhisperLabel" : "badge.ownShoutLabel")
    : t(`badge.${kind}Label`);
  // The leading space keeps the marker a separate word in the row's accessible name.
  return (
    <>
      {" "}
      <span className="whisper-badge" data-tooltip={label} role="img" aria-label={label}>
        <span aria-hidden="true">{word}</span>
      </span>
    </>
  );
}
