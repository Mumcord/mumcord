import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { setSettingsOpener } from "../settings/settingsOpener";
import { PluginsPalette } from "./PluginsPalette";
import { PluginsSection } from "./PluginsSection";
import { resetPluginStore } from "./pluginStore";

let api: MockApi;

beforeEach(async () => {
  await applyLanguage("en");
  resetPluginStore();
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
  setSettingsOpener(null);
});

describe("PluginsSection", () => {
  it("shows the tabs without Developer unless Developer mode is on", async () => {
    const { rerender } = render(<PluginsSection developerMode={false} />);
    await screen.findByRole("tab", { name: "Installed" });
    expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual([
      "Installed",
      "Browse",
      "Updates",
      "Repositories",
    ]);
    rerender(<PluginsSection developerMode />);
    expect(screen.getByRole("tab", { name: "Developer" })).toBeInTheDocument();
  });

  it("moves between tabs with the arrow keys", async () => {
    render(<PluginsSection developerMode={false} />);
    const installed = await screen.findByRole("tab", { name: "Installed" });
    installed.focus();
    fireEvent.keyDown(installed, { key: "ArrowRight" });
    const browse = screen.getByRole("tab", { name: "Browse" });
    expect(browse).toHaveAttribute("aria-selected", "true");
    expect(browse).toHaveFocus();
    expect(browse).toHaveAttribute("tabindex", "0");
  });

  it("shows the --no-plugins banner", async () => {
    vi.spyOn(api, "pluginsList").mockResolvedValue({ noPlugins: true, allOff: false, plugins: [] });
    render(<PluginsSection developerMode={false} />);
    expect(await screen.findByText(/started with --no-plugins/)).toBeInTheDocument();
  });

  it("turns all plugins off and reloads on plugins://changed", async () => {
    const setAllOff = vi.spyOn(api, "pluginsSetAllOff");
    const list = vi
      .spyOn(api, "pluginsList")
      .mockResolvedValue({ noPlugins: false, allOff: false, plugins: [] });
    render(<PluginsSection developerMode={false} />);
    fireEvent.click(await screen.findByRole("button", { name: "Turn off all plugins" }));
    expect(setAllOff).toHaveBeenCalledWith(true);
    list.mockResolvedValue({ noPlugins: false, allOff: true, plugins: [] });
    await act(async () => {
      api.mock.emit("pluginsChanged", {});
      await Promise.resolve();
    });
    expect(await screen.findByRole("button", { name: "Turn plugins on again" })).toBeVisible();
    expect(screen.getByText(/All plugins are turned off/)).toBeInTheDocument();
  });

  it("has no accessibility violations", async () => {
    const { container } = render(<PluginsSection developerMode />);
    await screen.findByRole("tab", { name: "Developer" });
    expect(await axeViolations(container)).toEqual([]);
  });
});

describe("PluginsPalette", () => {
  const open = () => {
    fireEvent.keyDown(window, { key: "P", ctrlKey: true, shiftKey: true });
  };

  it("opens Settings → Plugins from the palette", async () => {
    const opener = vi.fn();
    setSettingsOpener(opener);
    render(<PluginsPalette />);
    open();
    const input = await screen.findByRole("combobox", { name: "Command palette" });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(opener).toHaveBeenCalledWith("plugins");
    await waitFor(() => {
      expect(screen.queryByRole("combobox")).toBeNull();
    });
  });

  it("filters and turns off all plugins", async () => {
    const setAllOff = vi.spyOn(api, "pluginsSetAllOff");
    render(<PluginsPalette />);
    open();
    const input = await screen.findByRole("combobox", { name: "Command palette" });
    fireEvent.change(input, { target: { value: "turn off" } });
    expect(screen.getAllByRole("option")).toHaveLength(1);
    fireEvent.click(screen.getByRole("option", { name: "Turn off all plugins" }));
    expect(setAllOff).toHaveBeenCalledWith(true);
  });

  it("says so when nothing matches", async () => {
    render(<PluginsPalette />);
    open();
    const input = await screen.findByRole("combobox", { name: "Command palette" });
    fireEvent.change(input, { target: { value: "zzz" } });
    expect(screen.getByText("No matching command")).toBeInTheDocument();
  });
});
