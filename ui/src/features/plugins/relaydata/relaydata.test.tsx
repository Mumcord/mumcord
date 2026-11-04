import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createMockApi,
  setApi,
  type InstalledPlugin,
  type MockApi,
  type PluginRelayAvailability,
} from "../../../api";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { resetPlatformSettings } from "../platformSettings";
import { resetPluginStore } from "../pluginStore";
import { RelayDataChip } from "./RelayDataChip";
import { RelayDataGroup } from "./RelayDataGroup";

let api: MockApi;

function plugin(overrides: Partial<InstalledPlugin>): InstalledPlugin {
  return {
    id: "board-brawl",
    name: "Board Brawl",
    version: "1.0.0",
    source: { kind: "repo", repositoryId: 1, name: "Lantern" },
    status: { kind: "running" },
    enabled: true,
    disabledReason: null,
    required: [],
    optional: [{ id: "peers.relay", suggested: true }],
    granted: ["peers.relay"],
    visibility: "everyone",
    pinned: false,
    notifications: true,
    installedAt: null,
    badge: true,
    revocation: null,
    icon: null,
    description: "",
    authors: [],
    chatPatterns: [],
    offered: [],
    appMenuItems: 0,
    webSize: null,
    ...overrides,
  };
}

function availability(value: PluginRelayAvailability): void {
  vi.spyOn(api, "pluginRelayAvailability").mockResolvedValue(value);
}

beforeEach(async () => {
  await applyLanguage("en");
  resetPluginStore();
  resetPlatformSettings();
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("RelayDataChip", () => {
  it("shows the relay with its rate, then follows the availability event", async () => {
    availability({ kind: "available", kbps: 512 });
    render(<RelayDataChip serverId={1} />);
    const chip = await screen.findByText("Relay · end-to-end encrypted");
    expect(chip).toHaveAttribute("title", "up to 512 kbit/s per plugin");
    act(() => {
      api.mock.emit("pluginsRelayAvailability", { serverId: 1, availability: { kind: "noRelay" } });
    });
    expect(await screen.findByText("Mumble connection · slower")).toBeInTheDocument();
  });

  it("ignores other servers and shows the Mumble connection when the plugin cannot use the relay", async () => {
    availability({ kind: "available", kbps: null });
    render(<RelayDataChip serverId={1} usable={false} />);
    expect(await screen.findByText("Mumble connection · slower")).toBeInTheDocument();
    act(() => {
      api.mock.emit("pluginsRelayAvailability", { serverId: 2, availability: { kind: "noRelay" } });
    });
    expect(screen.queryByText("Relay · end-to-end encrypted")).toBeNull();
  });
});

describe("RelayDataGroup", () => {
  function setup(plugins: InstalledPlugin[]): void {
    vi.spyOn(api, "pluginsList").mockResolvedValue({ noPlugins: false, allOff: false, plugins });
    vi.spyOn(api, "pluginRelayUsage").mockResolvedValue([
      { pluginId: "board-brawl", bytes: 3_400_000 },
    ]);
    availability({ kind: "useNotAllowed" });
  }

  it("lists servers and plugin usage, and saves the global switch", async () => {
    setup([
      plugin({}),
      plugin({ id: "dice", name: "Dice roller", source: { kind: "official" }, optional: [] }),
    ]);
    const save = vi.spyOn(api, "pluginSetPlatformSettings");
    const view = render(<RelayDataGroup />);
    expect(await screen.findByText(/Unofficial · 3\.\d MB this session/)).toBeInTheDocument();
    expect(screen.getByText("Official · doesn't ask for relay data")).toBeInTheDocument();
    expect((await screen.findAllByText("Mumble connection only")).length).toBeGreaterThan(0);
    const global = await screen.findByRole("switch", { name: "Let plugins use a server's relay" });
    await waitFor(() => {
      expect(global).toBeEnabled();
    });
    fireEvent.click(global);
    await waitFor(() => {
      expect(save).toHaveBeenCalledWith(expect.objectContaining({ relayDataEnabled: false }));
    });
    expect(await axeViolations(view.container)).toEqual([]);
  });

  it("turns a plugin's relay permission off", async () => {
    setup([plugin({})]);
    const set = vi.spyOn(api, "pluginSetPermissions").mockResolvedValue(null);
    render(<RelayDataGroup />);
    fireEvent.click(await screen.findByRole("switch", { name: "Board Brawl may use the relay" }));
    await waitFor(() => {
      expect(set).toHaveBeenCalledWith("board-brawl", []);
    });
  });
});
