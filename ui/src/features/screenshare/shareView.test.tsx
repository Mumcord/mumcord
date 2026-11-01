/** Opening a watched share and the mini bar that remains (issue #163, FR-3.125). */
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi, ScreenShare } from "../../api";
import { connectedMock, resetAll } from "../channels/testUtils";
import { leaveCinema, mediaView, resetMediaView } from "../media/mediaView";
import { Viewer } from "./ScreenShareView";
import { ShareMiniBar } from "./ShareMiniBar";
import { resetScreenShareStore, shareStartedAt, useScreenShareOf } from "./sharesStore";
import { ViewerPanel } from "./ViewerPanel";
import { resetWatchingStore, startWatching } from "./watching";

// jsdom has no Tauri runtime; the frame channel only needs to exist for the viewer to render.
vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage: (message: ArrayBuffer) => void = () => undefined;
  },
}));

const ANNA = 2;

function annaShares(patch: Partial<ScreenShare> = {}): ScreenShare {
  return {
    session: ANNA,
    paused: false,
    sound: true,
    soundAvailable: false,
    viewers: [],
    blocked: null,
    relayReason: null,
    kind: "window",
    elapsedMs: 0,
    ...patch,
  };
}

describe("watched share", () => {
  let api: MockApi;

  beforeEach(async () => {
    api = await connectedMock({ bindStore: true });
    resetWatchingStore();
    resetMediaView();
    resetScreenShareStore();
  });

  afterEach(() => {
    vi.restoreAllMocks();
    resetAll();
  });

  it("opens in the main column, so nothing shows as the mini bar", () => {
    startWatching(1, ANNA);
    expect(mediaView().cinema).toBe("share");
    render(<ShareMiniBar serverId={1} />);
    expect(screen.queryByRole("group")).toBeNull();
  });

  it("remains as the mini bar after leaving, and Open goes back", () => {
    startWatching(1, ANNA);
    leaveCinema();
    render(<ShareMiniBar serverId={1} />);
    const bar = screen.getByRole("group", { name: /Screen share of/ });
    expect(bar).toHaveTextContent("Live");
    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    expect(mediaView().cinema).toBe("share");
  });

  it("names the path the picture travels over (FR-4.19)", async () => {
    vi.spyOn(api, "p2pLink").mockResolvedValue({ path: "relayUdp", ipHidden: false });
    const { container } = render(<Viewer serverId={1} session={ANNA} />);
    await vi.waitFor(() => {
      expect(container.querySelector(".share-path--relay")).toHaveTextContent("Relay");
    });
    fireEvent.click(screen.getByRole("button", { name: "Stream details" }));
    const details = screen.getByRole("group", { name: "Stream details" });
    expect(details).toHaveTextContent("Via relay (UDP) · IP visible");
    expect(details).toHaveTextContent("VP9 · Opus");
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("group", { name: "Stream details" })).toBeNull();
  });

  it("shows no path line while no connection to the sharer is open", async () => {
    const link = vi.spyOn(api, "p2pLink").mockResolvedValue(null);
    const { container } = render(<Viewer serverId={1} session={ANNA} />);
    await act(async () => {
      await link(1, ANNA);
    });
    expect(container.querySelector(".share-path")).toBeNull();
  });

  it("stop watching closes the share and the mini bar", () => {
    startWatching(1, ANNA);
    leaveCinema();
    render(<ShareMiniBar serverId={1} />);
    fireEvent.click(screen.getByRole("button", { name: "Stop watching" }));
    expect(mediaView().cinema).toBeNull();
    expect(screen.queryByRole("group")).toBeNull();
  });

  it("starts the share's clock when its announcement arrived", () => {
    vi.useFakeTimers({ now: 1_000_000 });
    let started = 0;
    function Probe() {
      const share = useScreenShareOf(1, ANNA);
      if (share !== null) started = shareStartedAt(share);
      return null;
    }
    render(<Probe />);
    act(() => {
      api.mock.emit("screenSharesChanged", {
        serverId: 1,
        shares: [annaShares({ elapsedMs: 60_000 })],
      });
    });
    expect(started).toBe(940_000);
    vi.useRealTimers();
  });

  it("panel names the sharer once and shows the keys", async () => {
    startWatching(1, ANNA);
    render(<ViewerPanel />);
    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: 1, shares: [annaShares()] });
    });
    const panel = await screen.findByRole("region", { name: /^Panel of / });
    expect(panel).toHaveTextContent(/Window\s·\ssound\son/);
    expect(panel).toHaveTextContent("Nobody is watching yet");
    expect(panel).toHaveTextContent("F fullscreen");
    const name = screen.getAllByText(/^Anna/);
    expect(name).toHaveLength(1);
  });

  it("Fit, Fill and 1:1 switch how the picture is shown", () => {
    render(<Viewer serverId={1} session={ANNA} />);
    const group = screen.getByRole("group", { name: "Picture size" });
    expect(group.querySelector("[aria-pressed='true']")).toHaveTextContent("Fit");
    fireEvent.click(screen.getByRole("button", { name: "Fill" }));
    expect(mediaView().fit).toBe("fill");
    expect(document.querySelector(".share-screen--fill")).not.toBeNull();
  });
});
