/** The own share on the voice server and whether the user may start one (FR-2.42, FR-2.65). */
import type { ScreenShare } from "../../api";
import { useServerStateOf, useVoiceServerId } from "../channels";
import {
  shareBlock,
  useDirectConsent,
  useSharePlatform,
  type ShareBlock,
  type SharePlatform,
} from "./availability";
import { useScreenShareOf } from "./sharesStore";

export interface OwnShareState {
  /** The voice server: shares always go to the user's voice channel. */
  serverId: number | null;
  own: ScreenShare | null;
  block: ShareBlock | null;
  platform: SharePlatform | null;
  /** Reads the consent again; resolves with the block that applies now. */
  recheck: () => Promise<ShareBlock | null>;
}

export function useOwnShare(): OwnShareState {
  const serverId = useVoiceServerId();
  const { connected, selfSession, users } = useServerStateOf(serverId);
  const own = useScreenShareOf(serverId, selfSession);
  const platform = useSharePlatform();
  const { consent, refresh } = useDirectConsent();
  const inChannel = connected && selfSession !== null && users.has(selfSession);
  const block = shareBlock({ inChannel, consent, platform });
  const recheck = async () => shareBlock({ inChannel, consent: await refresh(), platform });
  return { serverId, own, block, platform, recheck };
}
