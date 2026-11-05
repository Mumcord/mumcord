/**
 * One item of the media area (FR-9.44). Every item stays mounted while another one is the main
 * item, so switching tabs never stops a stream, a video or an activity; the hidden ones only stop
 * drawing and leave the keys to the main one.
 */
import type { ReactNode } from "react";
import { useMediaView, type StageId } from "./mediaView";
import { stageDomId, StageShown } from "./stageContext";

export function StagePanel({ id, children }: { id: StageId; children: ReactNode }) {
  const view = useMediaView();
  const shown = view.cinema === id;
  // Only a real tab row has tabs to point at; a single item is just the area's content.
  const tabbed = view.stage.length > 1;
  return (
    <div
      id={stageDomId("panel", id)}
      className="media-stage__panel"
      role={tabbed ? "tabpanel" : undefined}
      aria-labelledby={tabbed ? stageDomId("tab", id) : undefined}
      hidden={!shown}
    >
      <StageShown.Provider value={shown}>{children}</StageShown.Provider>
    </div>
  );
}
