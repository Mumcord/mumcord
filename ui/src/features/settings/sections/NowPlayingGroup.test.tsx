import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi, type Settings } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { NowPlayingGroup } from "./NowPlayingGroup";

let api: MockApi;
let settings: Settings;

beforeEach(async () => {
  await applyLanguage("en");
  api = createMockApi();
  setApi(api);
  settings = await api.settingsGet();
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
});

describe("NowPlayingGroup", () => {
  it("is off by default and turns sharing on", () => {
    const update = vi.fn();
    render(<NowPlayingGroup settings={settings} update={update} />);
    const toggle = screen.getByRole("switch", { name: "Share what I'm listening to" });
    expect(toggle).not.toBeChecked();
    fireEvent.click(toggle);
    expect(update).toHaveBeenCalledWith({ nowPlaying: true });
    expect(
      screen.getByRole("button", { name: "More information: Share what I'm listening to" }),
    ).toHaveAttribute("data-tooltip", expect.stringMatching(/official Mumble clients see/));
  });

  it("shows the detected track while on", async () => {
    vi.spyOn(api, "nowPlayingStatus").mockResolvedValue({
      supported: true,
      track: {
        title: "Song",
        artist: "Band",
        album: null,
        hasCover: false,
        positionMs: 0,
        durationMs: null,
        playing: true,
      },
    });
    render(<NowPlayingGroup settings={{ ...settings, nowPlaying: true }} update={vi.fn()} />);
    expect(await screen.findByText("Now sharing: Band – Song")).toBeInTheDocument();
  });

  it("explains that macOS is not supported", async () => {
    vi.spyOn(api, "nowPlayingStatus").mockResolvedValue({ supported: false, track: null });
    render(<NowPlayingGroup settings={{ ...settings, nowPlaying: true }} update={vi.fn()} />);
    expect(await screen.findByText(/Not supported on macOS/)).toBeInTheDocument();
    const toggle = screen.getByRole("switch", { name: "Share what I'm listening to" });
    expect(toggle).toBeDisabled();
    expect(toggle).not.toBeChecked();
  });
});
