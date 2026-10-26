import { useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import { shownServerId } from "../channels/serverStore";
import { useServerState } from "../channels";
import { describeFollowError } from "./errors";
import { useFollowState } from "./followStore";
import { setFollowNotice } from "./notice";
import { EyeIcon } from "./EyeIcon";

/** Eye: Follow / Following for a channel, or the FR-1.12 hint on servers before 1.4. */
export function FollowToggle({ channelId }: { channelId: number }) {
  const { t } = useTranslation("follow");
  const server = useServerState();
  const { followed } = useFollowState();
  const [busy, setBusy] = useState(false);
  // The error belongs to the channel it came from; it must not follow the selection around.
  const [failure, setFailure] = useState<{ channelId: number; text: string } | null>(null);
  const error = failure?.channelId === channelId ? failure.text : null;

  if (server.info && !server.info.followSupported) {
    return <p className="follow-hint">{t("unsupported")}</p>;
  }
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

  return (
    <>
      <button
        type="button"
        className="follow-toggle"
        aria-pressed={following}
        disabled={busy}
        onClick={() => {
          void toggle();
        }}
      >
        <EyeIcon />
        {following ? t("following") : t("follow")}
      </button>
      {error && (
        <p role="alert" className="follow-error">
          {error}
        </p>
      )}
    </>
  );
}
