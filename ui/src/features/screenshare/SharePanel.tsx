/**
 * Screen-share bar above the user panel: "Share screen" with its reason when it can't be used,
 * or the sharer's controls while sharing (FR-2.42–2.45).
 */
import { SharerControls } from "./SharerControls";
import { ShareScreenButton } from "./ShareScreenButton";
import { useOwnShare } from "./useOwnShare";
import "./screenShare.css";

export function SharePanel() {
  const share = useOwnShare();
  return (
    <div className="share-panel">
      {share.own === null ? <ShareScreenButton share={share} /> : <SharerControls share={share} />}
    </div>
  );
}
