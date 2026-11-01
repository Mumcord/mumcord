/** Opening a watched share and the mini bar that remains (issue #163, FR-3.125). */
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi } from "../../api";
import { connectedMock, resetAll } from "../channels/testUtils";
import { leaveCinema, mediaView, resetMediaView } from "../media/mediaView";
import { Viewer } from "./ScreenShareView";
import { ShareMiniBar } from "./ShareMiniBar";
import { resetScreenShareStore } from "./sharesStore";
import { resetWatchingStore, startWatching } from "./watching";

// jsdom has no Tauri runtime; the frame channel only needs to exist for the viewer to render.
vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage: (message: ArrayBuffer) => void = () => undefined;
  },
}));

const ANNA = 2;

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
      expect(container.querySelector(".p2p-link")?.textContent).toBe(
        "Via relay (UDP) · IP visible",
      );
    });
  });

  it("shows no path line while no connection to the sharer is open", async () => {
    const link = vi.spyOn(api, "p2pLink").mockResolvedValue(null);
    const { container } = render(<Viewer serverId={1} session={ANNA} />);
    await act(async () => {
      await link(1, ANNA);
    });
    expect(container.querySelector(".p2p-link")).toBeNull();
  });

  it("stop watching closes the share and the mini bar", () => {
    startWatching(1, ANNA);
    leaveCinema();
    render(<ShareMiniBar serverId={1} />);
    fireEvent.click(screen.getByRole("button", { name: "Stop watching" }));
    expect(mediaView().cinema).toBeNull();
    expect(screen.queryByRole("group")).toBeNull();
  });
});
