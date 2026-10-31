/** The own share on the voice server and whether the user may start one (FR-2.42, FR-2.65). */
import type { ScreenShare } from "../../api";
import { useServerStateOf, useVoiceServerId } from "../channels";
import {
  shareBlock,
  useP2pShareBlock,
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
  /** Asks the engine again; resolves with the block that applies now. */
  recheck: () => Promise<ShareBlock | null>;
}

export function useOwnShare(): OwnShareState {
  const serverId = useVoiceServerId();
  const { connected, selfSession, users } = useServerStateOf(serverId);
  const own = useScreenShareOf(serverId, selfSession);
  const platform = useSharePlatform();
  const { p2p, refresh } = useP2pShareBlock(serverId);
  const inChannel = connected && selfSession !== null && users.has(selfSession);
  const block = shareBlock({ inChannel, p2p, platform });
  const recheck = async () => shareBlock({ inChannel, p2p: await refresh(), platform });
  return { serverId, own, block, platform, recheck };
}
