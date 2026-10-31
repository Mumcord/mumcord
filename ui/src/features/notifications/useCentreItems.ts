import { useMemo } from "react";
import { useAllPmRequests } from "../requests/requestsStore";
import { buildItems, type CentreItem } from "./centreModel";
import { useCentreState } from "./centreStore";

/** Every entry of the notification centre, newest first, including the open requests (#141). */
export function useCentreItems(): CentreItem[] {
  const state = useCentreState();
  const requests = useAllPmRequests();
  return useMemo(() => buildItems(state, requests), [state, requests]);
}
