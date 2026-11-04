/**
 * The notices under the sharer's live bar (spec 07 §5 "Notices"). Each card decides on its own
 * when it shows.
 */
import type { ReactNode } from "react";
import type { ScreenShare, ShareQuality } from "../../../api";
import { EncoderSwitchNotice } from "./EncoderSwitchNotice";
import { FanoutNotice } from "./FanoutNotice";

export function ShareNotices({
  serverId,
  own,
  quality,
}: {
  serverId: number;
  own: ScreenShare;
  quality: ShareQuality | null;
}): ReactNode {
  return (
    <>
      <FanoutNotice serverId={serverId} own={own} quality={quality} />
      <EncoderSwitchNotice serverId={serverId} quality={quality} />
    </>
  );
}
