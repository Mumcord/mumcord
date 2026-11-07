/**
 * The feed (T5): newest first with its day groups (FR-11.10), the two-line row with the actor in
 * bold and "You" for the own session (FR-11.11), a live row that is a button with its action and
 * the live marker versus a plain row once it ended (FR-11.12), the plugin action routed back to the
 * plugin (FR-11.13) and the empty line (FR-11.14). The mock engine drives the events through the
 * store, so the rows are the ones the feed really builds.
 */
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi, ScreenShare, StageActivityView, User, WatchParty } from "../../api";
import { connectedMock, resetAll } from "../channels/testUtils";
import { resetMediaView } from "../media/mediaView";
import { resetActivityStore } from "../plugins/stage/activityStore";
import { resetScreenShareStore } from "../screenshare/sharesStore";
import { resetWatchingStore, watchedShare } from "../screenshare/watching";
import { resetWatchPartyStore } from "../watchparty/watchPartyStore";
import { resetActivityLogStore } from "./activityStore";
import { ActivityFeed } from "./ActivityFeed";
import { resetMembersTabStore } from "./membersTabStore";

const SERVER = 1;
const BEN = 3;
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

function party(host: number, channelId: number): WatchParty {
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

function activity(id: string, pluginName: string): StageActivityView {
  return {
    id,
    plugin: "brawl",
    pluginName,
    title: pluginName,
    starter: BEN,
    starterName: "Ben",
    participants: [BEN],
    max: 4,
    joined: false,
    startedByMe: false,
    web: false,
    kind: "activity",
  };
}

/** A local time, so the day groups are the days the user sees. */
function at(month: number, day: number, hour: number, minute: number): number {
  return new Date(2026, month, day, hour, minute).getTime();
}

const TODAY = at(10, 6, 14, 32);
const YESTERDAY = at(10, 5, 13, 45);
const timeOf = (at: number): string =>
  new Intl.DateTimeFormat("en", { timeStyle: "short" }).format(at);

/**
 * Drains the promises the stores started for their connect-time reads: the log's baseline is the
 * state at connect (FR-11.5), so an event is only recorded after it has landed.
 */
async function settle(): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

/** The row whose first line says `title`. */
function rowWith(title: string): HTMLElement {
  const row = screen.getAllByRole("listitem").find((item) => item.textContent.includes(title));
  if (row === undefined) throw new Error(`No feed row for "${title}"`);
  return row;
}

describe("activity feed", () => {
  let api: MockApi;

  beforeEach(async () => {
    // The saved history (#594) would carry events from one test into the next.
    localStorage.clear();
    resetActivityLogStore();
    resetMembersTabStore();
    resetScreenShareStore();
    resetWatchPartyStore();
    resetActivityStore();
    resetWatchingStore();
    resetMediaView();
    api = await connectedMock();
    // The mock's own data must not seed the baseline: the log then diffs everything after it.
    vi.spyOn(api, "screenShareList").mockResolvedValue([]);
    vi.spyOn(api, "watchPartyGet").mockResolvedValue(null);
    vi.spyOn(api, "stageActivities").mockResolvedValue([]);
    // Only the clock: `new Date()` and `Date.now()` then agree on the pinned day.
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(TODAY);
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
    resetActivityLogStore();
    resetMembersTabStore();
    resetScreenShareStore();
    resetWatchPartyStore();
    resetActivityStore();
    resetWatchingStore();
    resetMediaView();
    resetAll();
  });

  it("shows the empty line while nothing happened yet (FR-11.14)", () => {
    // The connect-time read has not landed, so the log is still empty.
    render(<ActivityFeed serverId={SERVER} />);
    expect(screen.getByText("No activity yet.")).toBeInTheDocument();
  });

  it("lists the newest event first and groups the rows by day (FR-11.10)", async () => {
    vi.setSystemTime(YESTERDAY);
    render(<ActivityFeed serverId={SERVER} />);
    await settle();
    vi.setSystemTime(TODAY);
    act(() => {
      api.mock.emit("userUpserted", { serverId: SERVER, user: person(NEWCOMER, "Nia", 1) });
    });
    expect(screen.getAllByRole("heading", { level: 3 }).map((day) => day.textContent)).toEqual([
      "Today",
      "Yesterday",
    ]);
    const rows = screen.getAllByRole("listitem").map((item) => item.textContent);
    expect(rows[0]).toContain("Nia connected");
    expect(rows[1]).toContain("You connected");
  });

  it("writes each row as two lines with the actor's name in bold (FR-11.11)", async () => {
    render(<ActivityFeed serverId={SERVER} />);
    await settle();
    // The first upsert of a session nobody knew before is a "connected" row; only a later channel
    // change of that same session is a move (FR-11.6).
    act(() => {
      api.mock.emit("userUpserted", { serverId: SERVER, user: person(NEWCOMER, "Nia", 1) });
    });
    act(() => {
      api.mock.emit("userUpserted", { serverId: SERVER, user: person(NEWCOMER, "Nia", 2) });
    });
    const row = rowWith("Nia moved channel");
    expect(within(row).getByText("Nia").tagName).toBe("STRONG");
    expect(within(row).getByText("moved channel")).toBeInTheDocument();
    expect(within(row).getByText(timeOf(TODAY))).toBeInTheDocument();
    expect(within(row).getByText("· # Raid")).toBeInTheDocument();
    // The own session reads as "You", not as its display name (FR-11.7).
    const own = rowWith("You connected");
    expect(within(own).getByText("You").tagName).toBe("STRONG");
  });

  it("keeps a live row clickable and turns it into a plain row once it ended (FR-11.12)", async () => {
    render(<ActivityFeed serverId={SERVER} />);
    await settle();
    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: SERVER, shares: [share(BEN)] });
    });
    const live = rowWith("Ben started a screen share");
    expect(within(live).getByRole("button")).toHaveAccessibleName("Watch Ben's screen share");
    expect(within(live).getByText("· live")).toBeInTheDocument();
    expect(within(live).getByText(/^Watch/)).toBeInTheDocument();

    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: SERVER, shares: [] });
    });
    const ended = rowWith("Ben started a screen share");
    expect(within(ended).queryByRole("button")).toBeNull();
    expect(within(ended).queryByText("· live")).toBeNull();
    expect(rowWith("Ben stopped a screen share")).not.toHaveAccessibleName(/^Watch/);
  });

  it("opens the flow of a live share, party or activity", async () => {
    render(<ActivityFeed serverId={SERVER} />);
    await settle();
    const joinParty = vi.spyOn(api, "watchPartyJoin").mockResolvedValue(party(BEN, 2));
    const joinStage = vi.spyOn(api, "stageJoin").mockResolvedValue(null);

    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: SERVER, shares: [share(BEN)] });
    });
    fireEvent.click(within(rowWith("Ben started a screen share")).getByRole("button"));
    expect(watchedShare()).toEqual({ serverId: SERVER, session: BEN });

    act(() => {
      api.mock.emit("watchPartyChanged", { serverId: SERVER, party: party(BEN, 2) });
    });
    fireEvent.click(within(rowWith("Ben started a watch party")).getByRole("button"));
    expect(joinParty).toHaveBeenCalledWith(SERVER);

    // The stage's first answer is the baseline; the second one is the change (FR-11.5), and the
    // activities belong to the voice server (FR-9.46).
    api.mock.emit("voiceServerChanged", { serverId: SERVER });
    act(() => {
      api.mock.emit("pluginsStageChanged", {});
    });
    await settle();
    vi.spyOn(api, "stageActivities").mockResolvedValue([activity("a2", "Board Brawl")]);
    act(() => {
      api.mock.emit("pluginsStageChanged", {});
    });
    await settle();
    const row = rowWith("Ben started Board Brawl");
    expect(within(row).getByRole("button")).toHaveAccessibleName("Join Ben's Board Brawl");
    fireEvent.click(within(row).getByRole("button"));
    expect(joinStage).toHaveBeenCalledWith("a2");
  });

  it("routes a plugin event's action back to the plugin (FR-11.13)", async () => {
    render(<ActivityFeed serverId={SERVER} />);
    await settle();
    const action = vi.spyOn(api, "pluginUiAction").mockResolvedValue(null);

    act(() => {
      api.mock.emit("activityEvent", {
        serverId: SERVER,
        pluginId: "fit",
        text: "Queue is open",
        actionLabel: "Notify me",
        actionId: "notify",
      });
    });
    const row = rowWith("fit “Queue is open”");
    expect(within(row).getByText("· plugin")).toBeInTheDocument();
    fireEvent.click(within(row).getByRole("button"));
    expect(action).toHaveBeenCalledWith("fit", null, "notify", { type: "none" }, SERVER);

    act(() => {
      api.mock.emit("activityEvent", {
        serverId: SERVER,
        pluginId: "fit",
        text: "plain line",
        actionLabel: null,
        actionId: null,
      });
    });
    expect(within(rowWith("fit “plain line”")).queryByRole("button")).toBeNull();
  });
});
