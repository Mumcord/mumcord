import { describe, expect, it } from "vitest";
import type { ServerState } from "../channels/serverStore";
import { INITIAL_READ_ONLY, nextReadOnly } from "./readOnly";

const SELF = 7;

/** Only the fields `nextReadOnly` reads; the rest of the state is irrelevant here. */
function stateWith(readOnly: boolean, selfDeaf: boolean): ServerState {
  return {
    selfSession: SELF,
    readOnly,
    users: new Map([[SELF, { session: SELF, selfDeaf }]]),
  } as unknown as ServerState;
}

describe("nextReadOnly on the first sync", () => {
  it("follows the engine's read-only flag, not the deafen flag", () => {
    // A link join restores a deafened choice: deafened but not read-only.
    expect(nextReadOnly(INITIAL_READ_ONLY, { kind: "synced" }, stateWith(false, true))).toEqual({
      readOnly: false,
      synced: true,
    });
    expect(
      nextReadOnly(INITIAL_READ_ONLY, { kind: "synced" }, stateWith(true, true)).readOnly,
    ).toBe(true);
  });

  it("keeps the state on a later sync (reconnect)", () => {
    const previous = { readOnly: true, synced: true };
    expect(nextReadOnly(previous, { kind: "synced" }, stateWith(false, false))).toBe(previous);
  });
});
