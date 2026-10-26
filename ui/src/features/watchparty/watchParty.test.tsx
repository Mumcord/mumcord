import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi, PlaylistEntry, WatchCommand, WatchParty } from "../../api";
import { clearPreviews } from "../chat/links";
import { flushServerUpdates, getServerState } from "../channels/serverStore";
import { selectConversation } from "../channels/selection";
import { connectedMock, resetAll } from "../channels/testUtils";
import { PartySync } from "./partySync";
import { ducker, RELEASE_DELAY_MS } from "./ducking";
import { VideoDucking } from "./VideoDucking";
import { MediaWidgetsMount } from "../../app/mounts";
import { NOTICE_MS, WatchPartyPanel } from "./WatchPartyPanel";
import { READY_TIMEOUT_MS } from "./WatchPartyPlayer";
import { WatchTogetherButton } from "./WatchTogetherButton";
import { positionAt, resetWatchPartyStore, type LiveParty } from "./watchPartyStore";
import { loadCommand, parseBridgeReport, playerOrigin } from "./playerBridge";
import { ENDED, PAUSED, PLAYING, isVideoId, videoIdOf, watchUrl } from "./youtube";

const VIDEO = "dQw4w9WgXcQ";
const ANNA = 2;
/** Where the mock API's player page lives. */
const PAGE_ORIGIN = "http://localhost:43215";

function party(patch: Partial<WatchParty> = {}): WatchParty {
  return {
    channelId: 0,
    videoId: VIDEO,
    playing: true,
    positionMs: 0,
    participants: [ANNA],
    lastAction: null,
    joined: false,
    currentEntry: 0,
    playlist: [],
    skippedBy: null,
    ...patch,
  };
}

describe("YouTube only", () => {
  it("accepts only video ids from youtube-nocookie embed URLs", () => {
    expect(videoIdOf(`https://www.youtube-nocookie.com/embed/${VIDEO}`)).toBe(VIDEO);
    expect(videoIdOf(`https://www.youtube-nocookie.com/embed/${VIDEO}?start=3`)).toBe(VIDEO);
    expect(videoIdOf(`https://evil.example/embed/${VIDEO}`)).toBeNull();
    expect(videoIdOf("https://www.youtube-nocookie.com/embed/../x")).toBeNull();
    expect(videoIdOf("https://player.twitch.tv/?channel=x")).toBeNull();
    expect(videoIdOf("not a url")).toBeNull();
    expect(isVideoId("javascript:")).toBe(false);
  });

  it("talks only to the loopback player pages, in typed messages", () => {
    const token = "0123456789abcdef0123456789abcdef";
    expect(playerOrigin(`http://localhost:5000/${token}/player.html`)).toBe(
      "http://localhost:5000",
    );
    expect(playerOrigin(`http://localhost:5000/${token}/embed.html`, "embed")).toBe(
      "http://localhost:5000",
    );
    expect(playerOrigin(`http://localhost:5000/${token}/embed.html`)).toBeNull();
    expect(playerOrigin(`http://localhost:5000/${token}/player.html?x=1`)).toBeNull();
    expect(playerOrigin(`https://localhost:5000/${token}/player.html`)).toBeNull();
    expect(playerOrigin(`http://evil.example:5000/${token}/player.html`)).toBeNull();
    expect(playerOrigin(`http://localhost/${token}/player.html`)).toBeNull();
    expect(playerOrigin(`http://localhost:5000/${token}/other.html`)).toBeNull();
    expect(playerOrigin("http://localhost:5000/short/player.html")).toBeNull();
    expect(playerOrigin("not a url")).toBeNull();

    expect(loadCommand(VIDEO)).toEqual({ type: "load", videoId: VIDEO });
    expect(loadCommand("x/../../y")).toBeNull();
    expect(watchUrl(VIDEO)).toBe(`https://www.youtube.com/watch?v=${VIDEO}`);
  });

  it("reads only well-formed reports from the player page", () => {
    expect(parseBridgeReport({ type: "ready" })).toEqual({ type: "ready" });
    expect(parseBridgeReport({ type: "ended", extra: 1 })).toEqual({ type: "ended" });
    expect(parseBridgeReport({ type: "state", state: 2 })).toEqual({ type: "state", state: 2 });
    expect(parseBridgeReport({ type: "time", seconds: 12.5 })).toEqual({
      type: "time",
      seconds: 12.5,
    });
    expect(parseBridgeReport({ type: "error", code: 153 })).toEqual({ type: "error", code: 153 });
    expect(parseBridgeReport({ type: "time", seconds: "x" })).toBeNull();
    expect(parseBridgeReport({ type: "state", state: Number.NaN })).toBeNull();
    expect(parseBridgeReport({ type: "play" })).toBeNull();
    expect(parseBridgeReport('{"type":"ready"}')).toBeNull();
    expect(parseBridgeReport(null)).toBeNull();
  });
});

describe("player sync", () => {
  let now = 0;
  let sent: WatchCommand[] = [];
  let calls: string[] = [];
  let ended: number[] = [];
  const sync = () =>
    new PartySync({
      now: () => now,
      player: {
        play: () => calls.push("play"),
        pause: () => calls.push("pause"),
        seekTo: (ms) => calls.push(`seek ${String(ms)}`),
      },
      send: (command) => sent.push(command),
      ended: (entry) => ended.push(entry),
    });
  const live = (patch: Partial<WatchParty>, atMs = now): LiveParty => ({
    party: party({ joined: true, ...patch }),
    atMs,
  });

  beforeEach(() => {
    now = 10_000;
    sent = [];
    calls = [];
    ended = [];
  });

  it("reports the end of the video with its playlist entry", () => {
    const s = sync();
    s.update(live({ currentEntry: 7 }));
    s.onReady();
    s.onState(ENDED);
    expect(ended).toEqual([7]);
    expect(sent).toEqual([]);
  });

  it("never replays an ended video, but plays the next one or a new Play", () => {
    const s = sync();
    s.update(live({ currentEntry: 2, positionMs: 100_000 }));
    s.onReady();
    s.onState(PLAYING);
    s.onTime(100);
    calls = [];
    s.onState(ENDED);
    expect(ended).toEqual([2]);
    now += 5000;
    s.onTime(100.2);
    s.update(live({ currentEntry: 2, positionMs: 105_000 }));
    expect(calls).not.toContain("play");
    expect(calls.filter((c) => c.startsWith("seek"))).toEqual([]);
    s.update(live({ currentEntry: 2, playing: false, positionMs: 100_200 }));
    expect(calls).toEqual([]);
    s.update(live({ currentEntry: 3, positionMs: 0 }));
    expect(calls).toContain("play");
    s.onState(ENDED);
    calls = [];
    const command = { kind: "play", positionMs: 0 } as const;
    s.update(live({ currentEntry: 3, positionMs: 0, lastAction: { session: ANNA, command } }));
    expect(calls).toContain("play");
  });

  it("sends a backward seek by dragging, which pauses during the drag, and keeps it", () => {
    // The engine echoes own commands at once, as watch_party_command does.
    let state = live({ positionMs: 100_000 });
    const s = new PartySync({
      now: () => now,
      player: {
        play: () => calls.push("play"),
        pause: () => calls.push("pause"),
        seekTo: (ms) => calls.push(`seek ${String(ms)}`),
      },
      send: (command) => {
        sent.push(command);
        const playing = command.kind === "seek" ? state.party.playing : command.kind === "play";
        const lastAction = { session: 1, command };
        state = live({ playing, positionMs: command.positionMs, lastAction });
        s.update(state);
      },
      ended: () => undefined,
    });
    s.update(state);
    s.onReady();
    now += 2000;
    s.onState(PLAYING);
    s.onTime(102);
    calls = [];
    now += 250;
    s.onState(PAUSED);
    now += 300;
    s.onTime(30);
    now += 250;
    s.onState(PLAYING);
    s.onTime(30);
    now += 3000;
    s.onTime(33);
    expect(sent).toEqual([
      { kind: "pause", positionMs: 102_250 },
      { kind: "seek", positionMs: 30_000 },
      { kind: "play", positionMs: 30_000 },
    ]);
    expect(calls.filter((c) => c.startsWith("seek"))).toEqual([]);
    expect(state.party).toMatchObject({ playing: true, positionMs: 30_000 });
  });

  it("follows a remote backward seek and stays there", () => {
    const s = sync();
    s.update(live({ positionMs: 100_000 }));
    s.onReady();
    s.onState(PLAYING);
    s.onTime(100);
    calls = [];
    const command = { kind: "seek", positionMs: 30_000 } as const;
    s.update(live({ positionMs: 30_000, lastAction: { session: ANNA, command } }));
    expect(calls).toContain("seek 30000");
    now += 250;
    s.onTime(100.25);
    now += 250;
    s.onTime(30.25);
    now += 2000;
    s.onTime(32.25);
    now += 250;
    s.onTime(32.5);
    expect(calls.filter((c) => c.startsWith("seek"))).toEqual(["seek 30000"]);
    expect(sent).toEqual([]);
  });

  it("starts a latecomer at the current position", () => {
    const s = sync();
    s.update(live({ positionMs: 60_000 }, 4_000));
    expect(calls).toEqual([]);
    s.onReady();
    expect(calls).toEqual(["seek 66000", "play"]);
    expect(positionAt(live({ positionMs: 60_000 }, 4_000), now)).toBe(66_000);
  });

  it("applies a remote pause without echoing it back", () => {
    const s = sync();
    s.update(live({ positionMs: 10_000 }));
    s.onReady();
    s.onState(PLAYING);
    s.onTime(10);
    now += 3000;
    s.onTime(13);
    calls = [];
    s.update(live({ playing: false, positionMs: 13_000 }));
    expect(calls).toEqual(["pause"]);
    s.onState(PAUSED);
    expect(sent).toEqual([]);
  });

  it("cues a freshly started party without playing it, but plays the next video", () => {
    const s = sync();
    s.update(live({ playing: false, positionMs: 0 }));
    s.onReady();
    expect(calls).toContain("pause");
    expect(calls).not.toContain("play");
    calls = [];
    s.update(live({ playing: true, positionMs: 0, currentEntry: 1 }));
    expect(calls).toContain("play");
  });

  it("turns the user's pause and seek into commands for everyone", () => {
    const s = sync();
    s.update(live({ positionMs: 20_000 }));
    s.onReady();
    now += 2000;
    s.onState(PLAYING);
    s.onTime(22);
    now += 500;
    s.onState(PAUSED);
    expect(sent).toEqual([{ kind: "pause", positionMs: 22_500 }]);
    s.update(live({ playing: false, positionMs: 22_500 }));
    now += 2000;
    s.onTime(22.5);
    now += 250;
    s.onTime(90);
    expect(sent.at(-1)).toEqual({ kind: "seek", positionMs: 90_000 });
  });
});

describe("panel", () => {
  let api: MockApi;
  let channelId: number;

  beforeEach(async () => {
    api = await connectedMock({ bindStore: true });
    const { users, selfSession } = getServerState();
    channelId = users.get(selfSession ?? -1)?.channelId ?? -1;
  });
  afterEach(() => {
    vi.useRealTimers();
    ducker.reset();
    resetWatchPartyStore();
    resetAll();
  });

  const announce = (p: WatchParty | null) => {
    act(() => {
      api.mock.emit("watchPartyChanged", { serverId: 1, party: p });
    });
  };

  it("invites without playing anything until Join is clicked", async () => {
    render(<WatchPartyPanel serverId={1} channelId={channelId} />);
    announce(party({ channelId }));
    expect(screen.getByRole("button", { name: "Join watch party" })).toBeInTheDocument();
    expect(screen.queryByTitle("Watch party player")).toBeNull();
    announce(party({ channelId, joined: true, participants: [ANNA, 1] }));
    expect(await screen.findByTitle("Watch party player")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Join watch party" })).toBeNull();
  });

  it("shows what someone else did for a few seconds, and nothing for own actions", () => {
    vi.useFakeTimers();
    render(<WatchPartyPanel serverId={1} channelId={channelId} />);
    const anna = getServerState().users.get(ANNA)?.name ?? "";
    announce(
      party({
        channelId,
        joined: true,
        playing: false,
        lastAction: { session: ANNA, command: { kind: "pause", positionMs: 65_000 } },
      }),
    );
    expect(screen.getByText(`${anna} paused`)).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(NOTICE_MS + 100);
    });
    expect(screen.queryByText(`${anna} paused`)).toBeNull();
    announce(
      party({
        channelId,
        joined: true,
        lastAction: { session: 1, command: { kind: "play", positionMs: 5000 } },
      }),
    );
    expect(document.querySelector(".watch-party__notice")).toBeNull();
  });

  it("shows nothing for another channel", () => {
    const { container } = render(<WatchPartyPanel serverId={1} channelId={channelId + 1000} />);
    announce(party({ channelId }));
    expect(container).toBeEmptyDOMElement();
  });

  it("offers the browser when the player does not answer", () => {
    vi.useFakeTimers();
    render(<WatchPartyPanel serverId={1} channelId={channelId} />);
    announce(party({ channelId, joined: true }));
    act(() => {
      vi.advanceTimersByTime(READY_TIMEOUT_MS + 100);
    });
    expect(screen.getByText("The player can't run here.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open in browser" })).toBeInTheDocument();
  });

  it("offers the browser when YouTube refuses to play here", async () => {
    render(<WatchPartyPanel serverId={1} channelId={channelId} />);
    announce(party({ channelId, joined: true }));
    const frame = await screen.findByTitle<HTMLIFrameElement>("Watch party player");
    act(() => {
      window.dispatchEvent(
        new MessageEvent("message", {
          origin: PAGE_ORIGIN,
          source: frame.contentWindow,
          data: { type: "error", code: 153 },
        }),
      );
    });
    expect(screen.getByRole("button", { name: "Open in browser" })).toBeInTheDocument();
    expect(screen.queryByTitle("Watch party player")).toBeNull();
  });

  it("drives the player through the loopback page and ignores other senders", async () => {
    render(<WatchPartyPanel serverId={1} channelId={channelId} />);
    announce(party({ channelId, joined: true, playing: false, positionMs: 30_000 }));
    const frame = await screen.findByTitle<HTMLIFrameElement>("Watch party player");
    expect(frame.src).toBe(`${PAGE_ORIGIN}/0123456789abcdef0123456789abcdef/player.html`);
    const target = frame.contentWindow;
    if (target === null) throw new Error("no frame window");
    const post = vi.spyOn(target, "postMessage").mockImplementation(() => undefined);
    fireEvent.load(frame);
    expect(post).toHaveBeenLastCalledWith({ type: "load", videoId: VIDEO }, PAGE_ORIGIN);

    const report = (origin: string, data: unknown) => {
      act(() => {
        window.dispatchEvent(new MessageEvent("message", { origin, source: target, data }));
      });
    };
    report("https://evil.example", { type: "ready" });
    report(PAGE_ORIGIN, '{"type":"ready"}');
    expect(frame).toHaveAttribute("data-ready", "false");
    report(PAGE_ORIGIN, { type: "ready" });
    expect(frame).toHaveAttribute("data-ready", "true");
    expect(post).toHaveBeenCalledWith({ type: "seek", seconds: 30 }, PAGE_ORIGIN);
    expect(post).toHaveBeenLastCalledWith({ type: "pause" }, PAGE_ORIGIN);
  });

  it("lowers the video while people in the channel talk, as the settings say (FR-2.76)", async () => {
    const level = vi.spyOn(api, "screenShareSetSoundLevel");
    act(() => {
      api.mock.emit("voiceServerChanged", { serverId: 1 });
    });
    render(
      <>
        <VideoDucking />
        <WatchPartyPanel serverId={1} channelId={channelId} />
      </>,
    );
    announce(party({ channelId, joined: true }));
    const frame = await screen.findByTitle<HTMLIFrameElement>("Watch party player");
    const target = frame.contentWindow;
    if (target === null) throw new Error("no frame window");
    const post = vi.spyOn(target, "postMessage").mockImplementation(() => undefined);
    act(() => {
      window.dispatchEvent(
        new MessageEvent("message", {
          origin: PAGE_ORIGIN,
          source: target,
          data: { type: "ready" },
        }),
      );
    });
    expect(post).not.toHaveBeenCalledWith(
      expect.objectContaining({ type: "setVolume" }),
      PAGE_ORIGIN,
    );

    vi.useFakeTimers();
    const selfSession = getServerState().selfSession ?? -1;
    const talk = (talking: boolean) => {
      act(() => {
        api.mock.emit("talkStateChanged", { serverId: 1, session: selfSession, talking });
        flushServerUpdates();
      });
    };
    talk(true);
    expect(post).toHaveBeenLastCalledWith({ type: "setVolume", level: 0.3 }, PAGE_ORIGIN);
    expect(level).toHaveBeenLastCalledWith(0.3);
    talk(false);
    act(() => {
      vi.advanceTimersByTime(RELEASE_DELAY_MS);
    });
    expect(post).toHaveBeenLastCalledWith({ type: "setVolume", level: 1 }, PAGE_ORIGIN);
    expect(level).toHaveBeenLastCalledWith(1);

    ducker.setSettings({ videoDucking: true, videoDuckingPercent: 60 });
    talk(true);
    expect(post).toHaveBeenLastCalledWith({ type: "setVolume", level: 0.6 }, PAGE_ORIGIN);
    talk(false);
    act(() => {
      vi.advanceTimersByTime(RELEASE_DELAY_MS);
    });

    ducker.setSettings({ videoDucking: false, videoDuckingPercent: 60 });
    post.mockClear();
    talk(true);
    expect(post).not.toHaveBeenCalledWith(
      expect.objectContaining({ type: "setVolume" }),
      PAGE_ORIGIN,
    );
  });

  it("starts a party from a YouTube card in the own channel and leaves it", async () => {
    act(() => {
      selectConversation({ kind: "channel", channelId });
      api.mock.emit("voiceServerChanged", { serverId: 1 });
    });
    render(
      <>
        <MediaWidgetsMount />
        <WatchTogetherButton
          playerUrl={`https://www.youtube-nocookie.com/embed/${VIDEO}`}
          title="Song"
          tabIndex={0}
        />
      </>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Watch Song together with this channel" }));
    expect(await screen.findByTitle("Watch party player")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Watch Song together/ })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Leave party" }));
    expect(await screen.findByRole("button", { name: /Watch Song together/ })).toBeInTheDocument();
  });

  it("offers no Watch together outside the own channel", () => {
    const other = [...getServerState().channels.keys()].find((id) => id !== channelId) ?? -1;
    act(() => {
      selectConversation({ kind: "channel", channelId: other });
    });
    const { container } = render(
      <WatchTogetherButton
        playerUrl={`https://www.youtube-nocookie.com/embed/${VIDEO}`}
        title="Song"
        tabIndex={0}
      />,
    );
    expect(container).toBeEmptyDOMElement();
  });
});

describe("playlist", () => {
  let api: MockApi;
  let channelId: number;
  const VIDEO_B = "a-b_c-d_e-f";
  const VIDEO_C = "0123456789a";
  const TITLES: Record<string, string> = { [VIDEO_B]: "Second song", [VIDEO_C]: "Third song" };

  const entry = (id: number, videoId: string, addedBy = ANNA): PlaylistEntry => ({
    id,
    videoId,
    addedBy,
    addedByName: null,
  });

  beforeEach(async () => {
    api = await connectedMock({ bindStore: true });
    const { users, selfSession } = getServerState();
    channelId = users.get(selfSession ?? -1)?.channelId ?? -1;
    vi.spyOn(api, "linkPreview").mockImplementation((url) => {
      const id = new URL(url).searchParams.get("v") ?? "";
      return Promise.resolve({
        url,
        siteName: "YouTube",
        title: TITLES[id] ?? null,
        description: null,
        image: "data:image/png;base64,AAAA",
        video: null,
        isImage: false,
      });
    });
  });
  afterEach(() => {
    vi.restoreAllMocks();
    clearPreviews();
    resetWatchPartyStore();
    resetAll();
  });

  const show = (patch: Partial<WatchParty>) => {
    act(() => {
      api.mock.emit("watchPartyChanged", {
        serverId: 1,
        party: party({ channelId, joined: true, participants: [ANNA, 1], ...patch }),
      });
    });
  };
  const anna = () => getServerState().users.get(ANNA)?.name ?? "";
  const upNextLine = () => screen.getByRole("button", { name: /^Up next/ });
  const openList = () => {
    fireEvent.mouseEnter(upNextLine());
  };
  const rows = () =>
    within(screen.getByRole("region", { name: "Up next" })).getAllByRole("listitem");

  it("shows title, thumbnail and who added each entry, in order", async () => {
    render(<WatchPartyPanel serverId={1} channelId={channelId} />);
    show({ playlist: [entry(5, VIDEO_C), { ...entry(4, VIDEO_B, 999), addedByName: "Gone" }] });
    await screen.findByText("Up next: Third song");
    openList();
    await screen.findByText("Third song");
    const [first, second] = rows();
    expect(first).toHaveTextContent(`Third song`);
    expect(
      within(first as HTMLElement).getByRole("img", { name: `Added by ${anna()}` }),
    ).toBeVisible();
    expect(within(second as HTMLElement).getByRole("img", { name: "Added by Gone" })).toBeVisible();
    expect(first?.querySelector("img")).toHaveAttribute("src", "data:image/png;base64,AAAA");
  });

  it("adds a video from the popover through the dialog, keeping the popover open", async () => {
    const add = vi.spyOn(api, "watchPartyAdd").mockResolvedValue(9);
    render(<WatchPartyPanel serverId={1} channelId={channelId} />);
    show({ playlist: [entry(4, VIDEO_B)] });
    await screen.findByText("Up next: Second song");
    openList();
    fireEvent.click(await screen.findByRole("button", { name: "Add video" }));
    const dialog = screen.getByRole("dialog");
    fireEvent.mouseLeave(upNextLine());
    await new Promise((resolve) => setTimeout(resolve, 250));
    expect(screen.getByRole("dialog")).toBe(dialog);
    expect(document.querySelector(".watch-playlist__popover")).not.toBeNull();
    const input = within(dialog).getByRole("textbox");
    fireEvent.change(input, { target: { value: `https://youtu.be/${VIDEO_C}` } });
    fireEvent.submit(input);
    expect(add).toHaveBeenCalledWith(1, VIDEO_C);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("removes and reorders entries by drag and by keyboard", async () => {
    const remove = vi.spyOn(api, "watchPartyRemove").mockResolvedValue(null);
    const move = vi.spyOn(api, "watchPartyMove").mockResolvedValue(null);
    render(<WatchPartyPanel serverId={1} channelId={channelId} />);
    show({ playlist: [entry(4, VIDEO_B), entry(5, VIDEO_C)] });
    await screen.findByText("Up next: Second song");
    openList();
    await screen.findByText("Third song");
    fireEvent.click(screen.getByRole("button", { name: "Remove Second song from the playlist" }));
    expect(remove).toHaveBeenCalledWith(1, 4);

    const [first, second] = rows();
    if (first === undefined || second === undefined) throw new Error("two rows expected");
    const dataTransfer = { setData: vi.fn(), effectAllowed: "" };
    fireEvent.dragStart(second, { dataTransfer });
    fireEvent.dragOver(first, { dataTransfer });
    fireEvent.drop(first, { dataTransfer });
    expect(move).toHaveBeenLastCalledWith(1, 5, 0);

    const handle = within(first).getByRole("button", { name: "Move Second song" });
    fireEvent.keyDown(handle, { key: "ArrowDown", altKey: true });
    expect(move).toHaveBeenLastCalledWith(1, 4, 1);
    move.mockClear();
    fireEvent.keyDown(handle, { key: "ArrowUp", altKey: true });
    fireEvent.keyDown(handle, { key: "ArrowDown" });
    expect(move).not.toHaveBeenCalled();
  });

  it("goes on to the next entry when the video ends", async () => {
    const next = vi.spyOn(api, "watchPartyNext").mockResolvedValue(null);
    render(<WatchPartyPanel serverId={1} channelId={channelId} />);
    show({ currentEntry: 3, playlist: [entry(4, VIDEO_B)] });
    const frame = await screen.findByTitle<HTMLIFrameElement>("Watch party player");
    act(() => {
      window.dispatchEvent(
        new MessageEvent("message", {
          origin: PAGE_ORIGIN,
          source: frame.contentWindow,
          data: { type: "ended" },
        }),
      );
    });
    await waitFor(() => {
      expect(next).toHaveBeenCalledWith(1, 3, false);
    });
  });

  it("loads a video added after the end instead of the ended one", async () => {
    vi.spyOn(api, "watchPartyNext").mockResolvedValue(null);
    render(<WatchPartyPanel serverId={1} channelId={channelId} />);
    show({ currentEntry: 3, positionMs: 200_000 });
    const ended = await screen.findByTitle<HTMLIFrameElement>("Watch party player");
    act(() => {
      window.dispatchEvent(
        new MessageEvent("message", {
          origin: PAGE_ORIGIN,
          source: ended.contentWindow,
          data: { type: "ended" },
        }),
      );
    });
    show({ currentEntry: 3, playing: false, positionMs: 200_000 });
    show({ currentEntry: 5, videoId: VIDEO_B, positionMs: 0 });
    const frame = screen.getByTitle<HTMLIFrameElement>("Watch party player");
    expect(frame).not.toBe(ended);
    const target = frame.contentWindow;
    if (target === null) throw new Error("no frame window");
    const post = vi.spyOn(target, "postMessage").mockImplementation(() => undefined);
    fireEvent.load(frame);
    expect(post).toHaveBeenLastCalledWith({ type: "load", videoId: VIDEO_B }, PAGE_ORIGIN);
  });

  it("shows icon controls and keeps the playlist hidden until hover or focus", async () => {
    render(<WatchPartyPanel serverId={1} channelId={channelId} />);
    show({ currentEntry: 3, playlist: [entry(4, VIDEO_B), entry(5, VIDEO_C)] });
    for (const name of ["Pause", "Skip"]) {
      expect(screen.getByRole("button", { name })).toHaveAttribute("data-tooltip", name);
    }
    const line = await screen.findByRole("button", { name: "Up next: Second song" });
    expect(screen.queryByRole("list")).toBeNull();
    fireEvent.mouseEnter(line);
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
    fireEvent.mouseLeave(line);
    await waitFor(() => {
      expect(screen.queryByRole("list")).toBeNull();
    });
    act(() => {
      line.focus();
    });
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
    fireEvent.keyDown(line, { key: "Escape" });
    expect(screen.queryByRole("list")).toBeNull();
    fireEvent.click(line);
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
  });

  it("skips for everyone and says who skipped", async () => {
    const next = vi.spyOn(api, "watchPartyNext").mockResolvedValue(null);
    render(<WatchPartyPanel serverId={1} channelId={channelId} />);
    show({ currentEntry: 3, playlist: [] });
    const skip = screen.getByRole("button", { name: "Skip" });
    expect(skip).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(skip);
    expect(next).not.toHaveBeenCalled();

    show({ currentEntry: 3, playlist: [entry(4, VIDEO_B)] });
    fireEvent.click(screen.getByRole("button", { name: "Skip" }));
    await waitFor(() => {
      expect(next).toHaveBeenCalledWith(1, 3, true);
    });
    show({ currentEntry: 4, videoId: VIDEO_B, skippedBy: ANNA });
    expect(screen.getByText(`${anna()} skipped`)).toBeInTheDocument();
    show({ currentEntry: 4, videoId: VIDEO_B, skippedBy: 1 });
    expect(screen.queryByText("You skipped")).toBeNull();
    expect(document.querySelector(".watch-party__notice")).toBeNull();
  });

  it("adds a card's video to the playlist while the user is in a party", async () => {
    const add = vi.spyOn(api, "watchPartyAdd").mockResolvedValue(9);
    act(() => {
      selectConversation({ kind: "channel", channelId });
    });
    render(
      <WatchTogetherButton
        playerUrl={`https://www.youtube-nocookie.com/embed/${VIDEO_B}`}
        title="Song"
        tabIndex={0}
      />,
    );
    show({ joined: false });
    expect(screen.queryByRole("button", { name: /Song/ })).toBeNull();
    show({});
    fireEvent.click(screen.getByRole("button", { name: "Add Song to the watch party playlist" }));
    await waitFor(() => {
      expect(add).toHaveBeenCalledWith(1, VIDEO_B);
    });
  });
});
