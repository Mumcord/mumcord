/**
 * "Live" marker next to a person who shares their screen (FR-2.45), in the channel tree and the
 * member list of the shown server. Only shares in the user's voice channel are known, so only
 * those are marked.
 */
import { useT } from "../../i18n/useT";
import { useShownServerId } from "../channels/serverStore";
import { Avatar } from "../members/Avatar";
import { useIsLive } from "./sharesStore";
import "./screenShare.css";

export function LiveBadge({ session }: { session: number }) {
  const t = useT("screenshare");
  const live = useIsLive(useShownServerId(), session);
  if (!live) return null;
  const label = t("liveLabel");
  // The leading space keeps the marker a separate word in the row's accessible name.
  return (
    <>
      {" "}
      <span className="live-badge" data-tooltip={label} role="img" aria-label={label}>
        <span aria-hidden="true">{t("live")}</span>
      </span>
    </>
  );
}

/** Avatar with the red live ring while the person shares their screen (FR-2.45). */
export function LiveAvatar({
  session,
  ...avatar
}: { session: number } & Omit<Parameters<typeof Avatar>[0], "live">) {
  const live = useIsLive(useShownServerId(), session);
  return <Avatar {...avatar} live={live} />;
}
