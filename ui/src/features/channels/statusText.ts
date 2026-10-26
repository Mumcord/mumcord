/** Status of a person as text, shared by the channel tree and the member list. */
import { useT } from "../../i18n/useT";
import type { User } from "../../api";

/**
 * The one voice state shown for a person. What the server imposes comes first, because the
 * person cannot undo it themselves: deafened by the server beats self-deafened, and a server
 * mute or suppression (no Speak permission in the channel) beats a self mute.
 */
export type VoiceState = "serverDeafened" | "deafened" | "serverMuted" | "suppressed" | "muted";

export function voiceState(user: User): VoiceState | null {
  if (user.serverDeaf) return "serverDeafened";
  if (user.selfDeaf) return "deafened";
  if (user.serverMute) return "serverMuted";
  if (user.suppressed) return "suppressed";
  if (user.selfMute) return "muted";
  return null;
}

/** Translation key (namespace "channels") of a voice state. */
export const VOICE_STATE_KEY = {
  serverDeafened: "status.serverDeafened",
  deafened: "status.deafened",
  serverMuted: "status.serverMuted",
  suppressed: "status.suppressed",
  muted: "status.muted",
} as const satisfies Record<VoiceState, string>;

/** Spoken status parts, e.g. ["talking", "muted by server"]. */
export function useStatusText() {
  const t = useT("channels");
  return (user: User, talking: boolean, listener = false): string[] => {
    const parts: string[] = [];
    // A listener row only marks the listening; the person's voice state belongs to their real row.
    if (listener) return [t("status.listening")];
    if (talking) parts.push(t("status.talking"));
    const voice = voiceState(user);
    if (voice) parts.push(t(VOICE_STATE_KEY[voice]));
    if (user.localMute) parts.push(t("status.localMuted"));
    if (!user.registered) parts.push(t("status.unregistered"));
    return parts;
  };
}
