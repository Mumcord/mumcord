import { useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import { shownServerId } from "../channels/serverStore";
import { useServerState } from "../channels";
import { describeFollowError } from "./errors";
import { useFollowState } from "./followStore";
import { setFollowNotice } from "./notice";

export interface FollowControl {
  /** False on servers before 1.4 (FR-1.12): following is hidden there. */
  supported: boolean;
  following: boolean;
  busy: boolean;
  /** Why the last toggle failed, for this channel only. */
  error: string | null;
  toggle: () => void;
}

/** Follow state of a channel and the action that flips it; shared by the eye and the channel menu. */
export function useFollowControl(channelId: number): FollowControl {
  const { t } = useTranslation("follow");
  const server = useServerState();
  const { followed } = useFollowState();
  const [busy, setBusy] = useState(false);
  // The error belongs to the channel it came from; it must not follow the selection around.
  const [failure, setFailure] = useState<{ channelId: number; text: string } | null>(null);
  const following = followed.includes(channelId);

  async function toggle() {
    setBusy(true);
    setFailure(null);
    // One refusal, one message: the tree's notice for the same channel would repeat it.
    setFollowNotice(null);
    try {
      const api = getApi();
      if (following) await api.channelUnfollow(shownServerId(), channelId);
      else await api.channelFollow(shownServerId(), channelId);
    } catch (e) {
      setFailure({ channelId, text: describeFollowError(t, e) });
    } finally {
      setBusy(false);
    }
  }

  return {
    supported: server.info?.followSupported ?? true,
    following,
    busy,
    error: failure?.channelId === channelId ? failure.text : null,
    toggle: () => {
      void toggle();
    },
  };
}
