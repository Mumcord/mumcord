/**
 * The notices under the sharer's live bar (spec 07 §5 "Notices"). Each card decides on its own
 * when it shows; the relay fan-out notice (FR-7.47, mockup 06) joins them here once the engine
 * reports why fan-out is unavailable.
 */
import type { ReactNode } from "react";
import type { ShareQuality } from "../../../api";
import { EncoderSwitchNotice } from "./EncoderSwitchNotice";

export function ShareNotices({
  serverId,
  quality,
}: {
  serverId: number;
  quality: ShareQuality | null;
}): ReactNode {
  return <EncoderSwitchNotice serverId={serverId} quality={quality} />;
}
