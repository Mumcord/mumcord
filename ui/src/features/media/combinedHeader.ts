/**
 * The combined card of a screen share or watch party with the camera strip (spec 08 §5.4,
 * mockups 08–09) has one header. The share or party view puts its own line ("Window · since
 * 07:44") and its chips into that header through these two places instead of drawing its own
 * overlay header, so the controls stay those of the view.
 */
import { createContext, useContext } from "react";

export interface CombinedHeaderSlots {
  /** Start of the header's second line, before "{n} cameras on". */
  sub: HTMLElement;
  /** Start of the header's chips, before the Beside / Above / Hidden switch. */
  chips: HTMLElement;
}

export const CombinedHeaderContext = createContext<CombinedHeaderSlots | null>(null);

/** The header places while the view is the main item of the combined card, else null. */
export function useCombinedHeader(): CombinedHeaderSlots | null {
  return useContext(CombinedHeaderContext);
}
