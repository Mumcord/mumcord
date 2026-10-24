import { useState, type KeyboardEvent, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import { useServerSelector, type ChannelAction } from "../channels";
import { describeFollowError } from "./errors";
import { useFollowState } from "./followStore";
import { setFollowNotice } from "./notice";
import "./follow.css";

/**
 * Star in a channel row (QA.13): outlined on hover/focus to follow, filled while followed.
 * Hidden on servers without follow support, like the chat header's Follow button.
 */
export function FollowStar({ channelId, name, tabbable }: ChannelAction) {
  const { t } = useTranslation("follow");
  // One star per channel row: select only the flag, so server events do not re-render them all.
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
      if (following) await api.channelUnfollow(channelId);
      else await api.channelFollow(channelId);
    } catch (e) {
      setFollowNotice({ text: describeFollowError(t, e), error: true });
    } finally {
      setBusy(false);
    }
  }

  // The row selects on click, joins on double-click and selects on Enter/Space; the star must not.
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
      // The glyph is CSS-generated: it must not join the treeitem's visible text (WCAG 2.5.3).
      tabIndex={tabbable ? 0 : -1}
      aria-disabled={busy}
      onClick={(event) => {
        event.stopPropagation();
        if (!busy) void toggle();
      }}
      onDoubleClick={stop}
      onKeyDown={onKeyDown}
    />
  );
}
