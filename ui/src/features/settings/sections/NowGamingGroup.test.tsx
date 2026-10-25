import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi, type Settings } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { NowGamingGroup } from "./NowGamingGroup";

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

describe("NowGamingGroup", () => {
  it("is off by default and turns sharing on", async () => {
    const update = vi.fn();
    render(<NowGamingGroup settings={settings} update={update} />);
    const toggle = screen.getByRole("switch", { name: "Share the game I'm playing" });
    expect(toggle).not.toBeChecked();
    fireEvent.click(toggle);
    expect(update).toHaveBeenCalledWith({ nowGaming: true });
    expect(await screen.findByText(/"🎮 Game" line/)).toBeInTheDocument();
  });

  it("shows the detected game while on", async () => {
    vi.spyOn(api, "nowGamingStatus").mockResolvedValue({
      supported: true,
      game: { name: "Factorio", hasIcon: true, playedMs: 0, canJoin: false },
    });
    render(<NowGamingGroup settings={{ ...settings, nowGaming: true }} update={vi.fn()} />);
    expect(await screen.findByText("Now sharing: Factorio")).toBeInTheDocument();
  });

  it("disables the switch where detection is unsupported", async () => {
    vi.spyOn(api, "nowGamingStatus").mockResolvedValue({ supported: false, game: null });
    render(<NowGamingGroup settings={{ ...settings, nowGaming: true }} update={vi.fn()} />);
    expect(await screen.findByText("Not supported on this system.")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Share the game I'm playing" })).toBeDisabled();
  });
});
