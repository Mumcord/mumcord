import { useMemo } from "react";
import { useIsBlocked } from "../blocking";
import { useAllPmRequests } from "../requests/requestsStore";
import { buildItems, type CentreItem } from "./centreModel";
import { useCentreState } from "./centreStore";

/** Every entry of the notification centre, newest first, including the open requests (#141),
 * without those of blocked people (#140). */
export function useCentreItems(): CentreItem[] {
  const state = useCentreState();
  const requests = useAllPmRequests();
  const isBlocked = useIsBlocked();
  return useMemo(() => buildItems(state, requests, isBlocked), [state, requests, isBlocked]);
}
