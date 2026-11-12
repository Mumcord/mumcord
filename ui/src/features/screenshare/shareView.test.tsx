/** Opening a watched share and the mini bar that remains (issue #163, FR-3.125). */
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { EngineError, type MockApi, type ScreenShare } from "../../api";
import { axeViolations } from "../../test/axe";
import { connectedMock, resetAll } from "../channels/testUtils";
import { CombinedHeaderContext } from "../media/combinedHeader";
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
    soundMode: "none",
    soundApp: null,
    soundReason: null,
    soundChoice: null,
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
    expect(details).toHaveTextContent("— · Opus");
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("group", { name: "Stream details" })).toBeNull();
  });

  it("puts its line and chips into the combined card's header (mockup 08)", async () => {
    vi.spyOn(api, "p2pLink").mockResolvedValue({ path: "relayUdp", ipHidden: false });
    const sub = document.createElement("span");
    const chips = document.createElement("span");
    document.body.append(sub, chips);
    const { container } = render(
      <CombinedHeaderContext value={{ sub, chips }}>
        <Viewer serverId={1} session={ANNA} />
      </CombinedHeaderContext>,
    );
    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: 1, shares: [annaShares()] });
    });
    // One header: the frame draws none of its own, the card's header gets the line and chips.
    expect(container.querySelector(".media-frame__top")).toBeNull();
    expect(sub.textContent).toMatch(/^Window · since \d{1,2}:\d{2}.* · $/);
    await vi.waitFor(() => {
      expect(chips.querySelector(".cam-chip[data-tone='ok']")).toHaveTextContent("Relay");
    });
    expect(container.querySelector(".cam-main__label")).toHaveTextContent("Anna's screenLive");
    // The chips stay the view's own controls.
    fireEvent.click(screen.getByRole("button", { name: "Stream details" }));
    expect(screen.getByRole("group", { name: "Stream details" })).toBeInTheDocument();
    sub.remove();
    chips.remove();
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

  it("closes the share's tab when the share ends, so the media area shows the rest", () => {
    startWatching(1, ANNA);
    expect(mediaView().cinema).toBe("share");
    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: 1, shares: [] });
    });
    expect(mediaView().stage).not.toContain("share");
    expect(mediaView().cinema).toBeNull();
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

  // The refusal texts of the 0.9.0 fallback (FR-7.99, FR-2.43, FR-2.46): the viewer shows why it
  // gets no picture instead of an empty stage, and every way out stays reachable by keyboard.
  it("says a share needs a newer Mumcord and offers no picture (FR-7.99)", async () => {
    const watch = vi
      .spyOn(api, "screenShareWatch")
      .mockRejectedValue(new EngineError("needsUpdate", ""));
    const { container } = render(<Viewer serverId={1} session={ANNA} />);
    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: 1, shares: [annaShares()] });
    });
    await screen.findByText("This share needs Mumcord 0.9.0.");
    // The stage's status line: a screen reader hears it without moving there.
    const status = screen.getByRole("status");
    expect(status).toHaveTextContent("This share needs Mumcord 0.9.0.");
    // No picture: the stage hides the empty canvas instead of announcing "Shared screen of Anna".
    expect(screen.queryByRole("img", { name: "Shared screen of Anna" })).toBeNull();
    fireEvent.click(within(status).getByRole("button", { name: "Try again" }));
    await vi.waitFor(() => {
      expect(watch).toHaveBeenCalledTimes(2);
    });
    expect(await axeViolations(container)).toEqual([]);
  });

  it("says the share is full (FR-2.43)", async () => {
    vi.spyOn(api, "screenShareWatch").mockRejectedValue(new EngineError("shareFull", ""));
    render(<Viewer serverId={1} session={ANNA} />);
    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: 1, shares: [annaShares()] });
    });
    expect(await screen.findByText("This share is full.")).toBeInTheDocument();
  });

  it("says the sharer's Mumcord is too old for direct connections (FR-2.46)", async () => {
    vi.spyOn(api, "screenShareWatch").mockRejectedValue(
      new EngineError("directUnavailable", "peerUnsupported"),
    );
    render(<Viewer serverId={1} session={ANNA} />);
    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: 1, shares: [annaShares()] });
    });
    expect(
      await screen.findByText("Anna's Mumcord is too old to share their screen with you."),
    ).toBeInTheDocument();
  });

  it("sets the share's own volume and leaves the sharer's voice volume alone (issue #333)", async () => {
    await api.personSetVolume(1, ANNA, 60);
    const setVoice = vi.spyOn(api, "personSetVolume");
    const setShare = vi.spyOn(api, "personSetShareVolume");
    render(<Viewer serverId={1} session={ANNA} />);
    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: 1, shares: [annaShares()] });
    });
    const slider = await screen.findByRole("slider", { name: "Volume" });
    expect(slider).toHaveValue("100");

    fireEvent.change(slider, { target: { value: "30" } });
    expect(setShare).toHaveBeenCalledWith(1, ANNA, 30);
    expect(setVoice).not.toHaveBeenCalled();
    const anna = (await api.sessionSnapshot(1))?.users.find((u) => u.session === ANNA);
    expect(anna).toMatchObject({ volume: 60, shareVolume: 30 });

    // The voice volume changing elsewhere leaves the share's slider where it was.
    await act(() => api.personSetVolume(1, ANNA, 150));
    expect(slider).toHaveValue("30");
  });
});
