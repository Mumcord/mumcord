/**
 * The non-React face of the share store (T3): the activity log follows the shares without a mounted
 * component, so its listener must fire from the engine events alone, and the getter must read the
 * same list the hooks read.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi, ScreenShare } from "../../api";
import { connectedMock, resetAll } from "../channels/testUtils";
import { resetScreenShareStore, sharesOf, subscribeShares } from "./sharesStore";

const SERVER = 1;
const ANNA = 2;

function share(patch: Partial<ScreenShare> = {}): ScreenShare {
  return {
    session: ANNA,
    paused: false,
    sound: false,
    soundAvailable: false,
    soundMode: "none",
    soundApp: null,
    soundReason: null,
    soundChoice: null,
    viewers: [],
    blocked: null,
    relayReason: null,
    kind: null,
    elapsedMs: 0,
    ...patch,
  };
}

describe("subscribeShares", () => {
  let api: MockApi;

  beforeEach(async () => {
    resetScreenShareStore();
    api = await connectedMock();
  });

  afterEach(() => {
    vi.restoreAllMocks();
    resetScreenShareStore();
    resetAll();
  });

  it("fires the listener on a change, and `sharesOf` reads the current list", () => {
    const listener = vi.fn();
    const unsubscribe = subscribeShares(listener);

    const started = share();
    api.mock.emit("screenSharesChanged", { serverId: SERVER, shares: [started] });
    expect(listener).toHaveBeenCalledTimes(1);
    expect(sharesOf(SERVER)).toEqual([started]);

    unsubscribe();
    api.mock.emit("screenSharesChanged", { serverId: SERVER, shares: [] });
    expect(listener).toHaveBeenCalledTimes(1);
    expect(sharesOf(SERVER)).toEqual([]);
  });
});
