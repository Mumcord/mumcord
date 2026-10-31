import type { ConnectionState } from "../../api";

/**
 * The one message of the user panel's status strip (#246). Persistent states (Do Not Disturb,
 * low latency, connection quality) are chips beside it instead, so they never compete for it.
 */
export type StripMessage =
  | "error"
  | "reconnecting"
  | "connecting"
  | "disconnected"
  | "readOnly"
  | "voiceElsewhere"
  | "voice";

export interface StripInput {
  error: string | null;
  /** Connection state of the shown server. */
  connection: ConnectionState["state"];
  /** The voice server, or the shown one while no server has voice, is connected and synced. */
  connected: boolean;
  voiceServerId: number | null;
  shownServerId: number | null;
}

/**
 * Picks the message by priority: an error until dismissed, then a connection in progress, not
 * connected, read-only (no voice anywhere), voice on another server, else the own voice channel.
 */
export function stripMessage(input: StripInput): StripMessage {
  if (input.error !== null) return "error";
  if (input.connection === "reconnecting") return "reconnecting";
  if (input.connection === "connecting") return "connecting";
  if (!input.connected) return "disconnected";
  if (input.voiceServerId === null) return "readOnly";
  if (input.voiceServerId !== input.shownServerId) return "voiceElsewhere";
  return "voice";
}
