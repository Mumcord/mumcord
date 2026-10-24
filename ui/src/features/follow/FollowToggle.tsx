import { useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import { useServerState } from "../channels";
import { describeFollowError } from "./errors";
import { useFollowState } from "./followStore";
import { EyeIcon } from "./EyeIcon";

/** Eye: Follow / Following for a channel, or the FR-1.12 hint on servers before 1.4. */
export function FollowToggle({ channelId }: { channelId: number }) {
  const { t } = useTranslation("follow");
  const server = useServerState();
  const { followed } = useFollowState();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (server.info && !server.info.followSupported) {
    return <p className="follow-hint">{t("unsupported")}</p>;
  }
  const following = followed.includes(channelId);

  async function toggle() {
    setBusy(true);
    setError(null);
    try {
      const api = getApi();
      if (following) await api.channelUnfollow(channelId);
      else await api.channelFollow(channelId);
    } catch (e) {
      setError(describeFollowError(t, e));
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
