/**
 * Read-only state (FR-1.14, FR-2.2): a fresh connect lands self-muted and deafened (reading
 * along) until the user explicitly joins a channel or undeafens, which makes that server the
 * voice server. Voice runs on exactly one server, so the user is read-only on every connected
 * server except the voice server. The engine owns the voice server (it also keeps it across an
 * automatic reconnect, FR-2.8), so the UI derives everything from it instead of guessing from the
 * deafen flag.
 */
import { getApi } from "../../api";
import { shownServerId, useShownServerId, useVoiceServerId } from "../channels/serverStore";

/** Explicit join of a channel on the shown server (double-click, Join button); moves voice there. */
export async function joinChannel(channelId: number): Promise<void> {
  await getApi().channelJoin(shownServerId(), channelId);
}

/** The user is read-only on the shown server: it is not the voice server. */
export function useReadOnly(): boolean {
  const shown = useShownServerId();
  return useVoiceServerId() !== shown;
}

/** No server has voice: the user is read-only everywhere (FR-2.5). */
export function useNoVoice(): boolean {
  return useVoiceServerId() === null;
}
