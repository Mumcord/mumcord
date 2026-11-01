import { describe, expect, it } from "vitest";
import type { MediaPopoutState } from "../bindings/commands";
import { createMockPopouts } from "./mockPopout";

const party = { serverId: 1, session: null };
const share = { serverId: 1, session: 7 };

function setup() {
  const seen: MediaPopoutState[] = [];
  const popouts = createMockPopouts((state) => seen.push(state));
  return { popouts, seen };
}

describe("mock media pop-outs", () => {
  it("starts docked", () => {
    const { popouts } = setup();
    expect(popouts.state()).toEqual({
      watch: { target: null, onTop: false },
      share: { target: null, onTop: false },
    });
  });

  it("pops out one kind and announces it once", () => {
    const { popouts, seen } = setup();
    popouts.open("share", share, null);
    popouts.open("share", share, null);
    expect(seen).toHaveLength(1);
    expect(seen[0]?.share.target).toEqual(share);
    expect(seen[0]?.watch.target).toBeNull();
  });

  it("docks again on close and keeps always on top", () => {
    const { popouts, seen } = setup();
    popouts.open("watch", party, true);
    popouts.close("watch");
    popouts.close("watch");
    expect(seen).toHaveLength(2);
    expect(popouts.state().watch).toEqual({ target: null, onTop: true });
    popouts.open("watch", party, null);
    expect(popouts.state().watch.onTop).toBe(true);
  });

  it("toggles always on top per kind", () => {
    const { popouts, seen } = setup();
    popouts.setOnTop("share", true);
    popouts.setOnTop("share", true);
    expect(seen).toHaveLength(1);
    expect(popouts.state()).toEqual({
      watch: { target: null, onTop: false },
      share: { target: null, onTop: true },
    });
  });
});
