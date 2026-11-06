/**
 * The non-React face of the watch party store (T3): the activity log follows the party without a
 * mounted component, so its listener must fire from the engine events alone, and the getter must
 * read the same party the hooks read.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi, WatchParty } from "../../api";
import { connectedMock, resetAll } from "../channels/testUtils";
import { resetWatchPartyStore, subscribeWatchParty, watchPartyOf } from "./watchPartyStore";

const SERVER = 1;
const ANNA = 2;
const VIDEO = "dQw4w9WgXcQ";

function party(patch: Partial<WatchParty> = {}): WatchParty {
  return {
    channelId: 0,
    videoId: VIDEO,
    playing: true,
    positionMs: 0,
    participants: [ANNA],
    lastAction: null,
    joined: true,
    currentEntry: 0,
    playlist: [],
    skippedBy: null,
    positions: [],
    host: 0,
    history: [],
    ...patch,
  };
}

describe("subscribeWatchParty", () => {
  let api: MockApi;

  beforeEach(async () => {
    resetWatchPartyStore();
    api = await connectedMock();
  });

  afterEach(() => {
    vi.restoreAllMocks();
    resetWatchPartyStore();
    resetAll();
  });

  it("fires the listener on a change, and `watchPartyOf` reads the current party", () => {
    const listener = vi.fn();
    const unsubscribe = subscribeWatchParty(listener);

    const started = party();
    api.mock.emit("watchPartyChanged", { serverId: SERVER, party: started });
    expect(listener).toHaveBeenCalledTimes(1);
    expect(watchPartyOf(SERVER)).toMatchObject({ party: started });

    unsubscribe();
    api.mock.emit("watchPartyChanged", { serverId: SERVER, party: null });
    expect(listener).toHaveBeenCalledTimes(1);
    expect(watchPartyOf(SERVER)).toBeNull();
  });
});
