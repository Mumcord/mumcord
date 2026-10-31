/**
 * The sharer's control bar (issue #163, FR-3.127): before a share it is the entry point, while
 * sharing it names the source and how long the share runs, shows the viewers, and drives pause and
 * stop through the engine.
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi, ScreenShare } from "../../api";
import { connectedMock, resetAll } from "../channels/testUtils";
import { resetScreenShareStore } from "./sharesStore";
import { SharerBar } from "./SharerBar";

const ANNA = 2;
const BEN = 3;
const BAR_NAME = "Your screen share";

describe("sharer control bar", () => {
  let api: MockApi;

  beforeEach(async () => {
    api = await connectedMock({ bindStore: true });
    api.mock.emit("voiceServerChanged", { serverId: 1 });
  });

  afterEach(() => {
    resetScreenShareStore();
    resetAll();
  });

  /** The own share with two viewers, as the engine reports it. */
  function watchedBy(own: ScreenShare): void {
    api.mock.emit("screenSharesChanged", { serverId: 1, shares: [own] });
  }

  it("is the entry point while nothing is shared", () => {
    render(<SharerBar />);
    expect(screen.getByRole("button", { name: /^Share screen/ })).toBeInTheDocument();
    expect(screen.getByRole("status", { name: BAR_NAME })).not.toHaveTextContent("Live");
  });

  it("shows the source, the running time and that it is live", async () => {
    render(<SharerBar />);
    await api.screenShareStart(1, false);
    const bar = screen.getByRole("status", { name: BAR_NAME });
    await vi.waitFor(() => {
      expect(bar).toHaveTextContent("Live");
      expect(bar).toHaveTextContent("1 min");
      expect(bar).toHaveTextContent("Shared picture");
      expect(bar).toHaveTextContent("Linux shares no sound");
    });
  });

  it("counts the viewers, and the ⋯ button names them with their quality", async () => {
    vi.spyOn(api, "screenShareQuality").mockResolvedValue({
      width: 1920,
      height: 1080,
      fps: 60,
      kbps: 4230,
      viewers: [
        { session: ANNA, fps: 60, struggling: false },
        { session: BEN, fps: 20, struggling: true },
      ],
    });
    render(<SharerBar />);
    await api.screenShareStart(1, false);
    const bar = screen.getByRole("status", { name: BAR_NAME });
    await vi.waitFor(() => expect(bar).toHaveTextContent("Nobody is watching yet"));
    watchedBy({
      session: 1,
      paused: false,
      sound: false,
      soundAvailable: false,
      viewers: [ANNA, BEN],
      blocked: null,
      kind: "window",
      elapsedMs: 60000,
    });
    await vi.waitFor(() => expect(bar).toHaveTextContent("2 watching: Anna, Ben"));
    fireEvent.click(screen.getByRole("button", { name: "Viewers" }));
    const popover = screen.getByRole("group", { name: "Viewers" });
    await vi.waitFor(() => {
      expect(popover).toHaveTextContent("60 fps, Direct");
      expect(popover).toHaveTextContent("Quality adapts to the slowest viewer");
    });
  });

  it("pause reaches the engine and shows the share as paused", async () => {
    render(<SharerBar />);
    await api.screenShareStart(1, false);
    const call = vi.spyOn(api, "screenShareSetPaused");
    fireEvent.click(screen.getByRole("button", { name: "Pause" }));
    expect(call).toHaveBeenCalledWith(1, true);
    await vi.waitFor(() =>
      expect(screen.getByRole("status", { name: BAR_NAME })).toHaveTextContent("Paused"),
    );
  });

  it("stop ends the share and the bar is the entry point again", async () => {
    render(<SharerBar />);
    await api.screenShareStart(1, false);
    fireEvent.click(screen.getByRole("button", { name: "Stop sharing" }));
    await vi.waitFor(() =>
      expect(screen.getByRole("button", { name: /^Share screen/ })).toBeInTheDocument(),
    );
  });
});
