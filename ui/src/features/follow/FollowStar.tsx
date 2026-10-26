import { useState, type KeyboardEvent, type MouseEvent } from "react";
import { useT } from "../../i18n/useT";
import { getApi } from "../../api";
import { shownServerId } from "../channels/serverStore";
import { useServerSelector, type ChannelAction } from "../channels";
import { describeFollowError } from "./errors";
import { useFollowState } from "./followStore";
import { setFollowNotice } from "./notice";
import { EyeIcon } from "./EyeIcon";
import "./follow.css";

/**
 * Eye in a channel row ("following = reading along") (QA.13): outlined on hover/focus to follow, filled while followed.
 * Hidden on servers without follow support, like the chat header's Follow button.
 */
export function FollowStar({ channelId, name, tabbable }: ChannelAction) {
  const t = useT("follow");
  // One eye per channel row: select only the flag, so server events do not re-render them all.
  const followSupported = useServerSelector((s) => s.info?.followSupported ?? false);
  const { followed } = useFollowState();
  const [busy, setBusy] = useState(false);
  if (!followSupported) return null;
  const following = followed.includes(channelId);

  async function toggle() {
    setBusy(true);
    setFollowNotice(null);
    try {
      const api = getApi();
      if (following) await api.channelUnfollow(shownServerId(), channelId);
      else await api.channelFollow(shownServerId(), channelId);
    } catch (e) {
      setFollowNotice({ text: describeFollowError(t, e), error: true });
    } finally {
      setBusy(false);
    }
  }

  // The row selects on click, joins on double-click and selects on Enter/Space; the eye must not.
  const stop = (event: MouseEvent) => {
    event.stopPropagation();
  };
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Enter" || event.key === " ") event.stopPropagation();
  };

  return (
    <button
      type="button"
      className="follow-star"
      data-followed={following}
      aria-label={t(following ? "star.unfollow" : "star.follow", { channel: name })}
      // The icon is an aria-hidden SVG without text, so it does not join the treeitem's visible text (WCAG 2.5.3).
      tabIndex={tabbable ? 0 : -1}
      aria-disabled={busy}
      onClick={(event) => {
        event.stopPropagation();
        if (!busy) void toggle();
      }}
      onDoubleClick={stop}
      onKeyDown={onKeyDown}
    >
      <EyeIcon />
    </button>
  );
}
