/**
 * The "this server has no relay" hint above the composer can be closed per server (#579). The
 * choice is kept on this device, so it survives restarts. When a relay appears on the server the
 * choice is forgotten, so the hint comes back if the relay goes away later.
 */
import { useCallback, useEffect, useRef, useState } from "react";

const KEY_PREFIX = "mumcord.noRelayHint.hidden.";

const keyOf = (serverId: number): string => `${KEY_PREFIX}${String(serverId)}`;

function readHidden(serverId: number): boolean {
  try {
    return localStorage.getItem(keyOf(serverId)) === "1";
  } catch {
    return false;
  }
}

function writeHidden(serverId: number, hidden: boolean): void {
  try {
    if (hidden) localStorage.setItem(keyOf(serverId), "1");
    else localStorage.removeItem(keyOf(serverId));
  } catch (error) {
    console.error("Saving the no-relay hint choice failed", error);
  }
}

/**
 * Whether the user closed the no-relay hint on this server, and the function that closes it.
 * `noRelay` is whether the server currently has no relay; `null` while that is unknown.
 */
export function useNoRelayHint(
  serverId: number | null,
  noRelay: boolean | null,
): { hidden: boolean; hide: () => void } {
  // The choice lives in storage; this only re-renders after closing.
  const [, setClosedCount] = useState(0);

  // A relay appeared while this server had none: forget the choice, so a later "no relay" shows
  // the hint again. Only a seen change counts; before the first answer nothing is known.
  const before = useRef<{ serverId: number | null; noRelay: boolean | null }>({
    serverId,
    noRelay,
  });
  useEffect(() => {
    const previous = before.current;
    before.current = { serverId, noRelay };
    if (serverId === null || previous.serverId !== serverId) return;
    if (previous.noRelay === true && noRelay === false) writeHidden(serverId, false);
  }, [serverId, noRelay]);

  const hide = useCallback(() => {
    if (serverId === null) return;
    writeHidden(serverId, true);
    setClosedCount((count) => count + 1);
  }, [serverId]);

  const hidden = serverId !== null && noRelay !== false && readHidden(serverId);
  return { hidden, hide };
}
