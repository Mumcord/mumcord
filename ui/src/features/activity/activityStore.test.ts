/**
 * The activity log store (T4): the baseline rule (FR-11.5), the derived events (FR-11.6, FR-11.7),
 * the cap (FR-11.8) and the discard at the end of a connection (FR-11.4). The mock drives both the
 * events and the connect-time reads the store makes for its baseline.
 */
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi, ScreenShare, StageActivityView, User, WatchParty } from "../../api";
import { createMockApi, setApi } from "../../api";
import { getServerStateOf } from "../channels/serverStore";
import { connectedMock, resetAll } from "../channels/testUtils";
import { resetActivityStore } from "../plugins/stage/activityStore";
import { resetScreenShareStore } from "../screenshare/sharesStore";
import { resetWatchPartyStore } from "../watchparty/watchPartyStore";
import {
  MAX_EVENTS,
  clearActivityHistory,
  setActivityHistoryKept,
  resetActivityLogStore,
  useActivityLog,
} from "./activityStore";

/** The default mock seed: server 1, Lobby is channel 1 and "Raid" channel 2, the own session is 1. */
const SERVER = 1;
const SELF = 1;
const ANNA = 2;
const BEN = 3;
const LOBBY = "Lobby";
const RAID = "Raid";
/** A session the seed does not have, so its first upsert reads as a new person. */
const NEWCOMER = 20;

function person(session: number, name: string, channelId: number): User {
  return {
    session,
    channelId,
    name,
    certHash: `${name.toLowerCase()}hash`,
    registered: true,
    selfMute: false,
    selfDeaf: false,
    serverMute: false,
    serverDeaf: false,
    suppressed: false,
    prioritySpeaker: false,
    listeningChannelIds: [],
    hasAvatar: false,
    avatarHash: null,
    hasComment: false,
    volume: 100,
    localMute: false,
    shareVolume: 100,
    isMumcord: false,
    mumcordFeatures: [],
    product: null,
    version: null,
    bot: null,
  };
}

function share(session: number): ScreenShare {
  return {
    session,
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
    kind: "window",
    elapsedMs: 126000,
  };
}

function party(host: number, channelId: number = 1): WatchParty {
  return {
    channelId,
    videoId: "video-1",
    playing: true,
    positionMs: 0,
    participants: [host],
    lastAction: null,
    joined: false,
    currentEntry: 0,
    playlist: [],
    skippedBy: null,
    positions: [],
    host,
    history: [],
  };
}

function activity(id: string, starter: number, name: string): StageActivityView {
  return {
    id,
    plugin: "brawl",
    pluginName: "Board Brawl",
    title: "Board Brawl",
    starter,
    starterName: name,
    participants: [starter],
    max: 4,
    joined: false,
    startedByMe: false,
    web: false,
    kind: "activity",
  };
}

/**
 * Drains the promises the stores started for their connect-time reads, so the log's baseline has
 * landed before an event is expected.
 */
async function settle(): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

describe("activity log", () => {
  let api: MockApi;

  beforeEach(async () => {
    // The saved history (#594) would carry events from one test into the next.
    localStorage.clear();
    resetActivityLogStore();
    resetScreenShareStore();
    resetWatchPartyStore();
    resetActivityStore();
    api = await connectedMock();
  });

  afterEach(() => {
    vi.restoreAllMocks();
    resetActivityLogStore();
    resetScreenShareStore();
    resetWatchPartyStore();
    resetActivityStore();
    resetAll();
  });

  it("records only 'You connected' for what was already running at connect (FR-11.5)", async () => {
    vi.spyOn(api, "screenShareList").mockResolvedValue([share(ANNA)]);
    vi.spyOn(api, "watchPartyGet").mockResolvedValue(party(ANNA));
    vi.spyOn(api, "stageActivities").mockResolvedValue([activity("a1", ANNA, "Anna")]);

    const { result } = renderHook(() => useActivityLog(SERVER));
    await settle();
    expect(result.current).toHaveLength(1);
    expect(result.current[0]).toMatchObject({
      kind: "connected",
      self: true,
      // The name is the display name the state holds for the own session (FR-11.7).
      name: getServerStateOf(SERVER).users.get(SELF)?.name,
    });
  });

  it("does not read a share or party that was already running at connect as started (AC-11.1)", async () => {
    // The demo seed delivers its running share and party right after the first sync, so the log
    // must take them as the baseline even though the stores reset themselves at the sync first.
    const demo = createMockApi({ seed: "demo" });
    setApi(demo);
    const { result } = renderHook(() => useActivityLog(1));
    await act(async () => {
      await demo.connect(1, null, null);
    });
    await settle();
    expect(result.current).toHaveLength(1);
    expect(result.current[0]).toMatchObject({ kind: "connected", self: true });
  });

  it("is empty for no server", async () => {
    const { result } = renderHook(() => useActivityLog(null));
    await settle();
    expect(result.current).toEqual([]);
  });

  it("makes no entries for bots joining, moving or leaving (#605)", async () => {
    const { result } = renderHook(() => useActivityLog(SERVER));
    await settle();
    const bot = { ...person(77, "Mumbot", 1), bot: { voice: true, modules: ["music"] } };
    act(() => {
      api.mock.emit("userUpserted", { serverId: SERVER, user: bot });
    });
    act(() => {
      api.mock.emit("userUpserted", { serverId: SERVER, user: { ...bot, channelId: 2 } });
    });
    act(() => {
      api.mock.emit("userRemoved", { serverId: SERVER, session: 77 });
    });
    expect(result.current.map((event) => event.kind)).toEqual(["connected"]);
  });

  it("derives connected, moved and disconnected from the people changes (FR-11.6)", async () => {
    const { result } = renderHook(() => useActivityLog(SERVER));
    await settle();

    act(() => {
      api.mock.emit("userUpserted", { serverId: SERVER, user: person(NEWCOMER, "Nia", 1) });
    });
    expect(result.current[0]).toMatchObject({ kind: "connected", name: "Nia", self: false });

    act(() => {
      api.mock.emit("userUpserted", { serverId: SERVER, user: person(NEWCOMER, "Nia", 2) });
    });
    expect(result.current[0]).toMatchObject({
      kind: "moved",
      name: "Nia",
      self: false,
      channelName: RAID,
    });

    act(() => {
      api.mock.emit("userRemoved", { serverId: SERVER, session: NEWCOMER });
    });
    expect(result.current[0]).toMatchObject({ kind: "disconnected", name: "Nia", self: false });
  });

  it("marks the own session as self (FR-11.7)", async () => {
    const { result } = renderHook(() => useActivityLog(SERVER));
    await settle();

    act(() => {
      api.mock.emit("userUpserted", {
        serverId: SERVER,
        user: { ...person(SELF, getServerStateOf(SERVER).users.get(SELF)?.name ?? "", 2) },
      });
    });
    expect(result.current[0]).toMatchObject({ kind: "moved", self: true, channelName: RAID });
  });

  it("diffs the screen shares after the baseline (FR-11.6)", async () => {
    vi.spyOn(api, "screenShareList").mockResolvedValue([share(ANNA)]);
    const { result } = renderHook(() => useActivityLog(SERVER));
    await settle();
    expect(result.current).toHaveLength(1);

    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: SERVER, shares: [share(ANNA), share(BEN)] });
    });
    expect(result.current[0]).toMatchObject({
      kind: "screenShare",
      started: true,
      name: "Ben",
      self: false,
      channelName: LOBBY,
      live: { kind: "screenShare", session: BEN },
    });

    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: SERVER, shares: [share(BEN)] });
    });
    // The share object is gone, so the row names the person the log remembered for it.
    expect(result.current[0]).toMatchObject({
      kind: "screenShare",
      started: false,
      name: "Anna",
      live: { kind: "screenShare", session: ANNA },
    });
  });

  it("diffs the watch party after the baseline (FR-11.6)", async () => {
    vi.spyOn(api, "watchPartyGet").mockResolvedValue(party(ANNA));
    const { result } = renderHook(() => useActivityLog(SERVER));
    await settle();
    expect(result.current).toHaveLength(1);

    // A delivery of the same party is not a change.
    act(() => {
      api.mock.emit("watchPartyChanged", { serverId: SERVER, party: party(ANNA) });
    });
    expect(result.current).toHaveLength(1);

    act(() => {
      api.mock.emit("watchPartyChanged", { serverId: SERVER, party: party(BEN, 2) });
    });
    expect(result.current[0]).toMatchObject({
      kind: "watchParty",
      started: true,
      name: "Ben",
      channelName: RAID,
      live: { kind: "watchParty" },
    });

    act(() => {
      api.mock.emit("watchPartyChanged", { serverId: SERVER, party: null });
    });
    expect(result.current[0]).toMatchObject({
      kind: "watchParty",
      started: false,
      name: "Ben",
      live: { kind: "watchParty" },
    });
  });

  it("diffs the plugin activities of the voice server after the baseline (FR-11.6)", async () => {
    // The stage keeps one list for the voice server, so the mock needs one: a fresh connect is
    // read-only and has no voice.
    await act(async () => {
      await api.setSelfDeaf(false);
    });
    const listed = vi.spyOn(api, "stageActivities");
    listed.mockResolvedValue([activity("a1", ANNA, "Anna")]);
    const { result } = renderHook(() => useActivityLog(SERVER));
    await settle();
    expect(result.current).toHaveLength(1);

    // A "Get plugin" notice is not an activity the log records (FR-11.6).
    listed.mockResolvedValue([
      activity("a1", ANNA, "Anna"),
      { ...activity("n1", ANNA, "Anna"), kind: "getPlugin" },
    ]);
    act(() => {
      api.mock.emit("pluginsStageChanged", {});
    });
    await settle();
    expect(result.current).toHaveLength(1);

    listed.mockResolvedValue([activity("a1", ANNA, "Anna"), activity("a2", BEN, "Ben")]);
    act(() => {
      api.mock.emit("pluginsStageChanged", {});
    });
    await settle();
    expect(result.current[0]).toMatchObject({
      kind: "pluginActivity",
      started: true,
      name: "Ben",
      pluginName: "Board Brawl",
      channelName: LOBBY,
      live: { kind: "pluginActivity", id: "a2" },
    });

    listed.mockResolvedValue([activity("a1", ANNA, "Anna")]);
    act(() => {
      api.mock.emit("pluginsStageChanged", {});
    });
    await settle();
    expect(result.current[0]).toMatchObject({
      kind: "pluginActivity",
      started: false,
      name: "Ben",
      // The activity is gone from the stage, so the name the event kept at event time is what the
      // feed can still show (FR-11.11).
      pluginName: "Board Brawl",
      live: { kind: "pluginActivity", id: "a2" },
    });
  });

  it("keeps 200 events per server and drops the oldest (FR-11.8)", async () => {
    const { result } = renderHook(() => useActivityLog(SERVER));
    await settle();

    act(() => {
      for (let session = 100; session < 100 + MAX_EVENTS + 1; session += 1) {
        api.mock.emit("userUpserted", {
          serverId: SERVER,
          user: person(session, `P${String(session)}`, 1),
        });
      }
    });
    expect(result.current).toHaveLength(MAX_EVENTS);
    expect(result.current[0]).toMatchObject({
      kind: "connected",
      name: `P${String(100 + MAX_EVENTS)}`,
    });
    // "You connected" was the oldest and is gone.
    expect(result.current.some((event) => event.kind === "connected" && event.self)).toBe(false);
  });

  it("keeps the log but stops diffing when the connection ends (#594)", async () => {
    const { result } = renderHook(() => useActivityLog(SERVER));
    await settle();
    expect(result.current).toHaveLength(1);

    act(() => {
      api.mock.emit("connectionStateChanged", {
        serverId: SERVER,
        state: { state: "disconnected", error: null },
      });
    });
    expect(result.current).toHaveLength(1);

    // Without a baseline nothing is collected any more, until the next sync.
    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: SERVER, shares: [share(ANNA)] });
    });
    expect(result.current).toHaveLength(1);
  });

  it("saves the log on this device and loads it as earlier after a restart (#594)", async () => {
    setActivityHistoryKept(true);
    const { result, unmount } = renderHook(() => useActivityLog(SERVER));
    await settle();
    expect(result.current).toHaveLength(1);
    const saved: unknown = JSON.parse(
      localStorage.getItem(`mumcord.activity.${String(SERVER)}`) ?? "null",
    );
    expect(saved).toEqual([expect.objectContaining({ kind: "connected", self: true })]);
    unmount();

    // A restart: the in-memory log is gone, the saved one is still there.
    resetActivityLogStore();
    setActivityHistoryKept(true);
    const reloaded = renderHook(() => useActivityLog(SERVER));
    expect(reloaded.result.current).toEqual([
      expect.objectContaining({ kind: "connected", self: true, earlier: true }),
    ]);
  });

  it("ignores an unreadable saved history (#594)", () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    localStorage.setItem(`mumcord.activity.${String(SERVER)}`, "{not json");
    resetActivityLogStore();
    setActivityHistoryKept(true);
    const { result } = renderHook(() => useActivityLog(SERVER));
    expect(result.current).toEqual([]);
  });

  it("saves nothing while the history is not kept, and deletes it when turned off (#661)", async () => {
    localStorage.setItem(
      `mumcord.activity.${String(SERVER)}`,
      JSON.stringify([{ kind: "connected", self: true, serverId: SERVER, at: 1 }]),
    );
    const { result } = renderHook(() => useActivityLog(SERVER));
    await settle();
    // Before the setting is known nothing is loaded and the saved feed is left alone.
    expect(result.current).toHaveLength(1);
    act(() => {
      setActivityHistoryKept(true);
    });
    expect(result.current).toHaveLength(2);
    act(() => {
      setActivityHistoryKept(false);
    });
    expect(localStorage.getItem(`mumcord.activity.${String(SERVER)}`)).toBeNull();
    expect(result.current).toHaveLength(2);
  });

  it("clears the feed and the saved history of every server (#594)", async () => {
    const { result } = renderHook(() => useActivityLog(SERVER));
    await settle();
    expect(result.current).toHaveLength(1);
    act(() => {
      clearActivityHistory();
    });
    expect(result.current).toEqual([]);
    expect(localStorage.getItem(`mumcord.activity.${String(SERVER)}`)).toBeNull();
  });

  it("appends plugin-pushed events and ignores a push without a server (FR-11.9)", async () => {
    const { result } = renderHook(() => useActivityLog(SERVER));
    await settle();

    act(() => {
      api.mock.emit("activityEvent", {
        serverId: SERVER,
        pluginId: "fit",
        text: "Queue is open",
        actionLabel: "Notify me",
        actionId: "notify",
      });
    });
    expect(result.current[0]).toMatchObject({
      kind: "pluginEvent",
      plugin: {
        pluginId: "fit",
        text: "Queue is open",
        actionLabel: "Notify me",
        actionId: "notify",
      },
    });
    expect(result.current).toHaveLength(2);

    act(() => {
      api.mock.emit("activityEvent", {
        serverId: null,
        pluginId: "fit",
        text: "no server",
        actionLabel: null,
        actionId: null,
      });
    });
    expect(result.current).toHaveLength(2);

    act(() => {
      api.mock.emit("activityEvent", {
        serverId: SERVER,
        pluginId: "fit",
        text: "plain line",
        actionLabel: null,
        actionId: null,
      });
    });
    expect(result.current).toHaveLength(3);
    expect(result.current[0]).toMatchObject({
      kind: "pluginEvent",
      plugin: { pluginId: "fit", text: "plain line", actionLabel: null, actionId: null },
    });
  });
});
