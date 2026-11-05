/** What a stage item's views need to know about the item around them (FR-9.44). */
import { createContext, useContext } from "react";
import type { StageId } from "./mediaView";

export const StageShown = createContext(true);

/** Whether the view around the caller is the main item; views outside the area always are. */
export function useStageShown(): boolean {
  return useContext(StageShown);
}

/** DOM id of an item's tab or panel; plugin ids carry characters ids should not have. */
export function stageDomId(part: "tab" | "panel", id: StageId): string {
  return `stage-${part}-${id.replace(/[^a-z0-9-]/gi, "-")}`;
}
