/** The cinema view's panel, header and the party store's adder memory (issue #163, FR-3.125). */
import { act, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi, PlaylistEntry, WatchParty } from "../../api";
import { renderWithEmbeds as render } from "../../app/testPreferences";
import { clearPreviews } from "../chat/links";
import { getServerState } from "../channels/serverStore";
import { connectedMock, resetAll } from "../channels/testUtils";
import { mediaView, openMedia, resetMediaView } from "../media/mediaView";
import { resetKnownDuration, setKnownDuration } from "./knownDuration";
import { WatchHeaderActions, WatchHeaderTitle } from "./WatchHeader";
import { WatchPartyPanel } from "./WatchPartyPanel";
import { resetWatchPartyStore, setWatchParty, useWatchParty } from "./watchPartyStore";

const VIDEO = "dQw4w9WgXcQ";
const VIDEO_B = "a-b_c-d_e-f";
const VIDEO_C = "0123456789a";
const TITLES: Record<string, string> = {
  [VIDEO]: "First song",
  [VIDEO_B]: "Second song",
  [VIDEO_C]: "Third song",
};
const ANNA = 2;

const entry = (id: number, videoId: string, addedBy = ANNA): PlaylistEntry => ({
  id,
  videoId,
  addedBy,
  addedByName: null,
});

function party(patch: Partial<WatchParty> = {}): WatchParty {
  return {
    channelId: 0,
    videoId: VIDEO,
    playing: true,
    positionMs: 0,
    participants: [ANNA, 1],
    lastAction: null,
    joined: true,
    currentEntry: 0,
    playlist: [],
    skippedBy: null,
    positions: [],
    host: ANNA,
    history: [],
    ...patch,
  };
}

describe("cinema panel and header", () => {
  let api: MockApi;
  let channelId: number;

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
        music: null,
        isImage: false,
        isAudio: false,
      });
    });
    resetMediaView();
    openMedia("watch");
  });
  afterEach(() => {
    vi.restoreAllMocks();
    clearPreviews();
    resetWatchPartyStore();
    resetMediaView();
    resetKnownDuration();
    resetAll();
  });

  const show = (patch: Partial<WatchParty>) => {
    act(() => {
      api.mock.emit("watchPartyChanged", { serverId: 1, party: party({ channelId, ...patch }) });
    });
  };
  const anna = () => getServerState().users.get(ANNA)?.name ?? "";

  it("shows Now playing with the host as adder and the length only there", async () => {
    render(<WatchPartyPanel serverId={1} channelId={channelId} />);
    show({ playlist: [entry(4, VIDEO_B)] });
    act(() => {
      setKnownDuration(VIDEO, 1_050_000);
    });
    const now = (await screen.findByText("First song")).closest(".party-now") as HTMLElement;
    expect(now).toHaveTextContent(anna());
    expect(now).toHaveTextContent("17:30");
    const row = document.querySelector<HTMLElement>(".party-entry");
    if (row === null) throw new Error("a row expected");
    expect(await within(row).findByText("Second song")).toBeVisible();
    expect(row).not.toHaveTextContent("17:30");
    expect(screen.getByRole("tab", { name: /Playlist/ })).toHaveTextContent("1");
    expect(screen.getByRole("heading", { name: "Who's watching" })).toBeVisible();
  });

  it("plays an entry next or removes it from its menu", async () => {
    const move = vi.spyOn(api, "watchPartyMove").mockResolvedValue(null);
    const remove = vi.spyOn(api, "watchPartyRemove").mockResolvedValue(null);
    render(<WatchPartyPanel serverId={1} channelId={channelId} />);
    show({ playlist: [entry(4, VIDEO_B), entry(5, VIDEO_C)] });
    await screen.findByText("Third song");
    fireEvent.click(screen.getByRole("button", { name: "More for Third song" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Play next" }));
    expect(move).toHaveBeenCalledWith(1, 5, 0);
    fireEvent.click(screen.getByRole("button", { name: "More for Second song" }));
    expect(screen.getByRole("menuitem", { name: "Play next" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    fireEvent.click(screen.getByRole("menuitem", { name: "Remove from playlist" }));
    expect(remove).toHaveBeenCalledWith(1, 4);
  });

  it("shows where a dragged entry lands and moves it there", async () => {
    const move = vi.spyOn(api, "watchPartyMove").mockResolvedValue(null);
    render(<WatchPartyPanel serverId={1} channelId={channelId} />);
    show({ playlist: [entry(4, VIDEO_B), entry(5, VIDEO_C)] });
    await screen.findByText("Third song");
    const [first, second] = document.querySelectorAll<HTMLElement>(".party-entry");
    if (first === undefined || second === undefined) throw new Error("two rows expected");
    const dataTransfer = { setData: vi.fn(), effectAllowed: "" };
    fireEvent.dragStart(first, { dataTransfer });
    fireEvent.dragOver(second, { dataTransfer });
    expect(second).toHaveAttribute("data-drop", "after");
    fireEvent.drop(second, { dataTransfer });
    expect(move).toHaveBeenCalledWith(1, 4, 1);
    expect(second).not.toHaveAttribute("data-drop");
  });

  it("puts the party pill and its actions into the channel header only while the cinema is open", () => {
    const leave = vi.spyOn(api, "watchPartyLeave").mockResolvedValue(null);
    render(
      <>
        <WatchHeaderTitle serverId={1}>
          <span>Lobby</span>
        </WatchHeaderTitle>
        <WatchHeaderActions serverId={1} />
      </>,
    );
    show({});
    expect(screen.getByText("Watch together · 2")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Mini player" }));
    expect(mediaView()).toMatchObject({ cinema: null, mini: { watch: true } });
    expect(screen.queryByText("Watch together · 2")).toBeNull();
    expect(screen.queryByRole("button", { name: "Leave party" })).toBeNull();
    expect(screen.getByText("Lobby")).toBeVisible();

    act(() => {
      openMedia("watch");
    });
    fireEvent.click(screen.getByRole("button", { name: "Leave party" }));
    expect(leave).toHaveBeenCalledWith(1);
  });
});

describe("who added the playing video", () => {
  afterEach(() => {
    resetWatchPartyStore();
  });

  function Adder() {
    const live = useWatchParty(1);
    return <p>{live?.currentAdder === null ? "unknown" : String(live?.currentAdder.session)}</p>;
  }

  it("is the host for the first video and is remembered when an entry moves up", () => {
    render(<Adder />);
    act(() => {
      setWatchParty(1, party({ host: 7, playlist: [entry(3, VIDEO_B, 9)] }));
    });
    expect(screen.getByText("7")).toBeVisible();
    act(() => {
      setWatchParty(1, party({ host: 7, currentEntry: 3, videoId: VIDEO_B, playlist: [] }));
    });
    expect(screen.getByText("9")).toBeVisible();
    act(() => {
      setWatchParty(1, party({ host: 7, currentEntry: 3, videoId: VIDEO_B, playing: false }));
    });
    expect(screen.getByText("9")).toBeVisible();
    act(() => {
      setWatchParty(1, party({ host: 7, currentEntry: 8, videoId: VIDEO_C }));
    });
    expect(screen.getByText("unknown")).toBeVisible();
  });
});
