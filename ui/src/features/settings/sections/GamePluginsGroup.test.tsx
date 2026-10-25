import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, EngineError, setApi, type MockApi } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { GamePluginsGroup } from "./GamePluginsGroup";

let api: MockApi;

beforeEach(async () => {
  await applyLanguage("en");
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
});

describe("GamePluginsGroup", () => {
  it("lists the plugins with the anti-cheat notice and switches one on", async () => {
    const setEnabled = vi.spyOn(api, "pluginSetEnabled");
    render(<GamePluginsGroup />);
    expect(screen.getByText(/anti-cheat/)).toBeInTheDocument();
    const toggle = await screen.findByRole("switch", { name: "Grand Theft Auto V" });
    expect(toggle).not.toBeChecked();
    fireEvent.click(toggle);
    expect(setEnabled).toHaveBeenCalledWith("gtav", true);
    expect(await screen.findByRole("switch", { name: "Grand Theft Auto V" })).toBeChecked();
  });

  it("shows which plugin delivers positions (FR-1.43)", async () => {
    vi.spyOn(api, "pluginsList").mockResolvedValue([
      {
        id: "gtav",
        name: "Grand Theft Auto V",
        version: "1.0.0",
        enabled: true,
        active: true,
        problem: null,
      },
    ]);
    render(<GamePluginsGroup />);
    await screen.findByRole("switch", { name: "Grand Theft Auto V" });
    act(() => {
      api.mock.emit("positionalStatusChanged", {
        status: { enabled: true, source: "Grand Theft Auto V", game: null, senders: [] },
      });
    });
    expect(screen.getByRole("status")).toHaveTextContent("Positions from: Grand Theft Auto V");
    expect(screen.getByText("Version 1.0.0 · Delivering positions")).toBeInTheDocument();
  });

  it("explains plugin problems and disables incompatible ones", async () => {
    vi.spyOn(api, "pluginsList").mockResolvedValue([
      {
        id: "old",
        name: "Old",
        version: null,
        enabled: false,
        active: false,
        problem: "incompatible",
      },
      {
        id: "bad",
        name: "Bad",
        version: null,
        enabled: true,
        active: false,
        problem: "initFailed",
      },
    ]);
    render(<GamePluginsGroup />);
    expect(await screen.findByRole("switch", { name: "Old" })).toBeDisabled();
    expect(screen.getByText("Not compatible with Mumcord.")).toBeInTheDocument();
    expect(screen.getByText("Failed to start.")).toBeInTheDocument();
  });

  it("says when no plugins are found and reports failures", async () => {
    vi.spyOn(api, "pluginsList").mockResolvedValue([]);
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    render(<GamePluginsGroup />);
    expect(await screen.findByText(/No game plugins found/)).toBeInTheDocument();

    vi.spyOn(api, "pluginsList").mockRejectedValue(new EngineError("internal", "stuck"));
    render(<GamePluginsGroup />);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The game plugins could not be listed.",
    );
  });
});
