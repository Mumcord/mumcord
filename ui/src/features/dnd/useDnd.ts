import { useCallback, useEffect, useState } from "react";
import { getApi, type DndState } from "../../api";

/** Why Do Not Disturb is in effect: the Mumcord switch, the followed system state, or not at all. */
export type DndSource = "manual" | "system" | null;

export function dndSource(state: DndState | null): DndSource {
  if (!state) return null;
  if (state.manual) return "manual";
  if (state.followSystem && state.system.on === true) return "system";
  return null;
}

export interface DndControls {
  /** `null` while loading. */
  state: DndState | null;
  setManual: (on: boolean) => void;
  setFollowSystem: (on: boolean) => void;
}

/**
 * Do Not Disturb (FR-3.111, FR-3.112): the switch, "Follow system" and the system state. Follows
 * changes from anywhere (tray, another view, the desktop) live.
 */
export function useDnd(): DndControls {
  const [state, setState] = useState<DndState | null>(null);

  useEffect(() => {
    let active = true;
    const off = getApi().on("dndChanged", (event) => {
      setState(event.state);
    });
    getApi()
      .dndState()
      .then(
        (loaded) => {
          if (active) setState(loaded);
        },
        (error: unknown) => {
          console.error("Reading the Do Not Disturb state failed", error);
        },
      );
    return () => {
      active = false;
      off();
    };
  }, []);

  const run = useCallback((change: Promise<DndState>) => {
    change.then(setState, (error: unknown) => {
      console.error("Changing Do Not Disturb failed", error);
    });
  }, []);

  const setManual = useCallback(
    (on: boolean) => {
      run(getApi().dndSet(on));
    },
    [run],
  );
  const setFollowSystem = useCallback(
    (on: boolean) => {
      run(getApi().dndSetFollowSystem(on));
    },
    [run],
  );

  return { state, setManual, setFollowSystem };
}
