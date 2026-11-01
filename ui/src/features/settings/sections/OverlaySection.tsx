import { useMemo } from "react";
import { getApi } from "../../../api";
import { InGameOverlayGroup } from "./InGameOverlayGroup";
import { createInGameOverlayApi, type InGameOverlayApi } from "./inGameOverlay";

/**
 * The in-game overlay card (FR-1.44 – FR-1.46b). `inGameApi` defaults to the engine's in-game
 * overlay commands; tests pass their own.
 */
export function InGameOverlayCard({ inGameApi }: { inGameApi?: InGameOverlayApi }) {
  const engineInGameApi = useMemo(() => createInGameOverlayApi(getApi()), []);
  return <InGameOverlayGroup api={inGameApi ?? engineInGameApi} />;
}
