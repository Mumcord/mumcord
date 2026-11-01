/** Popping the media views out into their own window and back (issue #164). */
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi, WatchParty } from "../../api";
import { renderWithEmbeds } from "../../app/testPreferences";
import { getServerState } from "../channels/serverStore";
import { connectedMock, resetAll } from "../channels/testUtils";
import { resetFrameHandover } from "../screenshare/frameHandover";
import { ScreenShareView, Viewer } from "../screenshare/ScreenShareView";
import { resetScreenShareStore } from "../screenshare/sharesStore";
import { resetWatchingStore, startWatching, stopWatching } from "../screenshare/watching";
import { WatchTogetherView } from "../watchparty/WatchTogetherView";
import { resetWatchPartyStore, setWatchParty } from "../watchparty/watchPartyStore";
import { MediaPopoutHost } from "./MediaPopoutHost";
import { leaveCinema, mediaView, openMedia, resetMediaView } from "./mediaView";
import { InPopout, isPoppedOut, popoutKindOf, popouts, resetPopoutStore } from "./popoutStore";

// jsdom has no Tauri runtime; the frame channel only needs to exist for the viewer to render.
vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage: (message: ArrayBuffer) => void = () => undefined;
  },
}));

const ANNA = 2;
const SHARE = { serverId: 1, session: ANNA };

function party(): WatchParty {
  return {
    channelId: 0,
    videoId: "dQw4w9WgXcQ",
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
  };
}

describe("media pop-out", () => {
  let api: MockApi;

  beforeEach(async () => {
    api = await connectedMock({ bindStore: true });
    resetPopoutStore();
    resetFrameHandover();
    resetWatchingStore();
    resetWatchPartyStore();
    resetScreenShareStore();
    resetMediaView();
    vi.spyOn(api, "screenShareWatch").mockResolvedValue(null);
    vi.spyOn(api, "screenShareUnwatch").mockResolvedValue(null);
    vi.spyOn(api, "screenShareFramesAttach").mockResolvedValue(null);
    vi.spyOn(api, "screenShareSetDecodeFallback").mockResolvedValue(null);
  });

  afterEach(() => {
    vi.restoreAllMocks();
    resetAll();
  });

  /** Opens the pop-out as the app does, which announces the new state. */
  async function popOutShare(): Promise<void> {
    await act(async () => {
      await api.mediaPopoutOpen("share", SHARE, "title", null);
    });
  }

  async function popInShare(): Promise<void> {
    await act(async () => {
      await api.mediaPopoutClose("share");
    });
  }

  it("reads the kind of the window from its address", () => {
    expect(popoutKindOf("?window=media-popout&kind=watch")).toBe("watch");
    expect(popoutKindOf("?window=media-popout&kind=share")).toBe("share");
    expect(popoutKindOf("?window=media-popout&kind=chat")).toBeNull();
  });

  it("follows the state the app announces", async () => {
    expect(isPoppedOut(popouts(), "share", 1, ANNA)).toBe(false);
    await popOutShare();
    expect(isPoppedOut(popouts(), "share", 1, ANNA)).toBe(true);
    expect(isPoppedOut(popouts(), "share", 1, 3)).toBe(false);
    await popInShare();
    expect(isPoppedOut(popouts(), "share", 1, ANNA)).toBe(false);
  });

  it("pops the share out with a title naming the sharer and the channel", () => {
    const open = vi.spyOn(api, "mediaPopoutOpen");
    const { users, channels, selfSession } = getServerState();
    const channel = channels.get(users.get(selfSession ?? -1)?.channelId ?? -1)?.name ?? "";
    const name = users.get(ANNA)?.name ?? "";
    render(<Viewer serverId={1} session={ANNA} />);
    fireEvent.click(screen.getByRole("button", { name: "Pop out" }));
    expect(open).toHaveBeenCalledWith("share", SHARE, `${name}'s screen — ${channel}`, null);
  });

  it("keeps the watch open while popped out and takes the frames back on docking", async () => {
    startWatching(1, ANNA);
    render(<ScreenShareView />);
    await vi.waitFor(() => {
      expect(api.screenShareWatch).toHaveBeenCalledTimes(1);
    });

    await popOutShare();
    expect(screen.getByRole("status")).toHaveTextContent("Playing in separate window");
    expect(api.screenShareUnwatch).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Bring back" }));
    await vi.waitFor(() => {
      expect(api.screenShareFramesAttach).toHaveBeenCalledTimes(1);
    });
    expect(api.screenShareWatch).toHaveBeenCalledTimes(1);
    expect(api.screenShareUnwatch).not.toHaveBeenCalled();
  });

  it("stops the kept watch and closes the window when the user stops watching", async () => {
    const close = vi.spyOn(api, "mediaPopoutClose");
    startWatching(1, ANNA);
    render(
      <>
        <ScreenShareView />
        <MediaPopoutHost />
      </>,
    );
    await vi.waitFor(() => {
      expect(api.screenShareWatch).toHaveBeenCalled();
    });
    await popOutShare();
    act(() => {
      stopWatching();
    });
    expect(api.screenShareUnwatch).toHaveBeenCalledWith(1, ANNA);
    await vi.waitFor(() => {
      expect(close).toHaveBeenCalledWith("share");
    });
  });

  it("brings the view back into the main column when the window closes", async () => {
    startWatching(1, ANNA);
    render(<MediaPopoutHost />);
    await popOutShare();
    act(() => {
      leaveCinema();
    });
    expect(mediaView().cinema).toBeNull();
    await popInShare();
    expect(mediaView().cinema).toBe("share");
  });

  it("only attaches in the pop-out, never starts or stops the watch", async () => {
    const close = vi.spyOn(api, "mediaPopoutClose");
    const { unmount } = render(
      <InPopout value>
        <Viewer serverId={1} session={ANNA} />
      </InPopout>,
    );
    await vi.waitFor(() => {
      expect(api.screenShareFramesAttach).toHaveBeenCalledTimes(1);
    });
    expect(screen.queryByRole("button", { name: "Stop watching" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Pop out" })).toBeNull();
    const onTop = screen.getByRole("button", { name: "Always on top" });
    expect(onTop).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(screen.getByRole("button", { name: "Pop back in" }));
    expect(close).toHaveBeenCalledWith("share");
    unmount();
    expect(api.screenShareWatch).not.toHaveBeenCalled();
    expect(api.screenShareUnwatch).not.toHaveBeenCalled();
  });

  it("turns always on top on from the pop-out", async () => {
    render(
      <InPopout value>
        <Viewer serverId={1} session={ANNA} />
      </InPopout>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Always on top" }));
    await vi.waitFor(() => {
      expect(screen.getByRole("button", { name: "Always on top" })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
    });
  });

  it("unloads the docked party player while the party plays in its own window", async () => {
    setWatchParty(1, party());
    openMedia("watch");
    const open = vi.spyOn(api, "mediaPopoutOpen");
    const { container } = renderWithEmbeds(<WatchTogetherView serverId={1} />);
    fireEvent.click(screen.getByRole("button", { name: "Pop out" }));
    expect(open).toHaveBeenCalledWith(
      "watch",
      { serverId: 1, session: null },
      expect.stringContaining("Watch party"),
      null,
    );
    await act(async () => {
      await open.mock.results[0]?.value;
    });
    expect(screen.getByRole("status")).toHaveTextContent("Playing in separate window");
    expect(container.querySelector("iframe")).toBeNull();
  });

  it("closes the party's window when the user leaves the party", async () => {
    const close = vi.spyOn(api, "mediaPopoutClose");
    setWatchParty(1, party());
    render(<MediaPopoutHost />);
    await act(async () => {
      await api.mediaPopoutOpen("watch", { serverId: 1, session: null }, "title", null);
    });
    act(() => {
      setWatchParty(1, null);
    });
    await vi.waitFor(() => {
      expect(close).toHaveBeenCalledWith("watch");
    });
    expect(mediaView().cinema).toBeNull();
  });
});
