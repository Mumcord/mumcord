/**
 * Media pop-outs of the mock engine (issue #164): the browser has no second window, so opening
 * and closing only change the state and announce it, as the app does once its window is up or
 * gone. The pop-out page itself can be tried with `?window=media-popout&kind=watch|share`.
 */
import type { MediaPopoutKind, MediaPopoutState, MediaPopoutTarget } from "../bindings/commands";

export interface MockPopouts {
  state(): MediaPopoutState;
  open(kind: MediaPopoutKind, target: MediaPopoutTarget, onTop: boolean | null): void;
  close(kind: MediaPopoutKind): void;
  setOnTop(kind: MediaPopoutKind, on: boolean): void;
}

export function createMockPopouts(announce: (state: MediaPopoutState) => void): MockPopouts {
  let state: MediaPopoutState = {
    watch: { target: null, onTop: false },
    share: { target: null, onTop: false },
  };

  function update(kind: MediaPopoutKind, change: Partial<MediaPopoutState[MediaPopoutKind]>): void {
    const before = state[kind];
    const after = { ...before, ...change };
    const sameTarget =
      before.target?.serverId === after.target?.serverId &&
      before.target?.session === after.target?.session;
    if (sameTarget && before.onTop === after.onTop) return;
    state = { ...state, [kind]: after };
    announce(state);
  }

  return {
    state: () => state,
    open: (kind, target, onTop) => {
      update(kind, { target, onTop: onTop ?? state[kind].onTop });
    },
    close: (kind) => {
      update(kind, { target: null });
    },
    setOnTop: (kind, on) => {
      update(kind, { onTop: on });
    },
  };
}
