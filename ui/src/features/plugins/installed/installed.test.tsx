import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type InstalledPlugin, type MockApi } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { setSettingsOpener } from "../../settings/settingsOpener";
import {
  resetPluginRequestStore,
  routePluginRequest,
  useRequestedPluginTab,
} from "../pluginRequestStore";
import { resetPluginStore } from "../pluginStore";
import { InstalledTab } from "./InstalledTab";
import { initials, permissionChips, sourceLabel, stateLabel } from "./labels";

let api: MockApi;

function plugin(overrides: Partial<InstalledPlugin>): InstalledPlugin {
  return {
    id: "echolings",
    name: "Echolings",
    version: "1.2.0",
    source: { kind: "official" },
    status: { kind: "running" },
    enabled: true,
    disabledReason: null,
    required: ["ui.panel"],
    optional: [{ id: "ui.member-badge", suggested: true }],
    granted: ["ui.panel", "ui.member-badge"],
    visibility: "everyone",
    pinned: false,
    notifications: true,
    installedAt: null,
    badge: true,
    revocation: null,
    icon: null,
    description: "",
    authors: [],
    offered: [],
    ...overrides,
  };
}

const echolings = plugin({});
const foo = plugin({
  id: "foo",
  name: "foo",
  version: "2.0.0",
  source: { kind: "repo", repositoryId: 1, name: "Foo Repo" },
  status: { kind: "autoOff", error: "trap" },
  enabled: false,
  disabledReason: "crashes",
  required: ["peers.messages"],
  optional: [],
  granted: ["peers.messages"],
  badge: false,
});

beforeEach(async () => {
  await applyLanguage("en");
  resetPluginStore();
  resetPluginRequestStore();
  api = createMockApi();
  setApi(api);
  Element.prototype.scrollIntoView = vi.fn();
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
  setSettingsOpener(null);
});

function withPlugins(plugins: InstalledPlugin[]): void {
  vi.spyOn(api, "pluginsList").mockResolvedValue({ noPlugins: false, allOff: false, plugins });
}

describe("labels", () => {
  it("names the source of every plugin", () => {
    expect(sourceLabel(echolings).key).toBe("labels.official");
    expect(sourceLabel(foo)).toMatchObject({
      key: "labels.unofficial",
      values: { repository: "Foo Repo" },
    });
    expect(sourceLabel(plugin({ source: { kind: "dev", folder: "/x" } })).key).toBe(
      "labels.developer",
    );
  });

  it("explains why a plugin is off", () => {
    expect(stateLabel(echolings)).toBeNull();
    expect(stateLabel(foo)?.key).toBe("labels.autoOff");
    expect(
      stateLabel(plugin({ enabled: false, disabledReason: "revoked", revocation: "Leaks data" })),
    ).toMatchObject({ key: "labels.revoked", values: { reason: "Leaks data" } });
    expect(stateLabel(plugin({ disabledReason: "tampered" }))?.key).toBe("labels.tampered");
    expect(stateLabel(plugin({ revocation: "Old" }))?.key).toBe("labels.revocationWarning");
  });

  it("builds initials", () => {
    expect(initials("Dice roller")).toBe("DR");
    expect(initials("foo")).toBe("FO");
  });
});

describe("InstalledTab", () => {
  it("shows rows with labels, the badge picker and no violations", async () => {
    withPlugins([echolings, foo]);
    const { container } = render(<InstalledTab />);
    expect(await screen.findByText("Official")).toBeInTheDocument();
    expect(screen.getByText("Unofficial · Foo Repo")).toBeInTheDocument();
    expect(screen.getByText("Turned off after repeated crashes")).toBeInTheDocument();
    expect(screen.getByText("Badge shown next to names")).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("switches a plugin on and off", async () => {
    withPlugins([echolings]);
    const setEnabled = vi.spyOn(api, "pluginSetEnabled").mockResolvedValue(null);
    render(<InstalledTab />);
    fireEvent.click(await screen.findByRole("switch", { name: "Echolings on" }));
    await waitFor(() => {
      expect(setEnabled).toHaveBeenCalledWith("echolings", false);
    });
  });

  it("shows an empty state that opens Browse", async () => {
    withPlugins([]);
    setSettingsOpener(vi.fn());
    const tab = { current: null as string | null };
    function Probe() {
      tab.current = useRequestedPluginTab();
      return null;
    }
    render(
      <>
        <InstalledTab />
        <Probe />
      </>,
    );
    expect(await screen.findByText("No plugins yet.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Browse plugins" }));
    expect(tab.current).toBe("browse");
  });

  it("marks a plugin with new permissions to answer and opens Updates", async () => {
    withPlugins([plugin({ offered: ["chat.triggers"] })]);
    setSettingsOpener(vi.fn());
    const tab = { current: null as string | null };
    function Probe() {
      tab.current = useRequestedPluginTab();
      return null;
    }
    render(
      <>
        <InstalledTab />
        <Probe />
      </>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "New permissions" }));
    expect(tab.current).toBe("updates");
  });

  it("changes permissions, visibility and notifications in the details", async () => {
    withPlugins([foo]);
    const setNotifications = vi.spyOn(api, "pluginSetNotifications").mockResolvedValue(null);
    render(<InstalledTab />);
    fireEvent.click(await screen.findByRole("button", { name: "Permissions", description: "foo" }));
    expect(screen.getByText("Who can see this plugin's activity")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("switch", { name: "Notifications" }));
    await waitFor(() => {
      expect(setNotifications).toHaveBeenCalledWith("foo", false);
    });
  });

  it("says which plugin took one of its commands", async () => {
    withPlugins([foo]);
    vi.spyOn(api, "pluginCommands").mockResolvedValue([
      {
        pluginId: "foo",
        pluginName: "Foo",
        name: "roll",
        description: "",
        usage: null,
        takenBy: "Dice",
      },
    ]);
    render(<InstalledTab />);
    fireEvent.click(await screen.findByRole("button", { name: "Permissions", description: "foo" }));
    expect(await screen.findByText("/roll: Command taken by Dice")).toBeInTheDocument();
  });

  it("removes a plugin and keeps its data unless asked", async () => {
    withPlugins([echolings]);
    const remove = vi.spyOn(api, "pluginRemove").mockResolvedValue(null);
    render(<InstalledTab />);
    fireEvent.click(await screen.findByRole("button", { name: "Remove" }));
    const dialog = screen.getByRole("dialog", { name: "Remove Echolings?" });
    expect(
      within(dialog).getByRole("checkbox", { name: "Also delete its data" }),
    ).not.toBeChecked();
    fireEvent.click(within(dialog).getByRole("checkbox", { name: "Also delete its data" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Remove" }));
    await waitFor(() => {
      expect(remove).toHaveBeenCalledWith("echolings", true);
    });
  });

  it("shows description, permission chips and an update hint with a direct update button", async () => {
    withPlugins([
      plugin({
        id: "dice",
        name: "Dice Roller",
        version: "0.4.1",
        description: "Roll dice together.",
        authors: ["Pip Larsen"],
        required: ["ui.chat-card"],
        optional: [],
        granted: ["ui.chat-card"],
      }),
    ]);
    vi.spyOn(api, "pluginUpdates").mockResolvedValue([
      {
        id: "dice",
        name: "Dice Roller",
        from: "0.4.1",
        to: "0.5.0",
        source: { kind: "official" },
        size: 10,
        approval: null,
        failed: null,
      },
    ]);
    const apply = vi.spyOn(api, "pluginApplyUpdate").mockResolvedValue(null);
    render(<InstalledTab />);
    expect(await screen.findByText("Roll dice together.")).toBeInTheDocument();
    expect(screen.getByText("0.4.1 · Pip Larsen")).toBeInTheDocument();
    expect(screen.getByText("Chat cards")).toBeInTheDocument();
    expect(await screen.findByText("Update 0.5.0")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Update", description: "Dice Roller" }));
    await waitFor(() => {
      expect(apply).toHaveBeenCalledWith("dice", ["ui.chat-card"]);
    });
  });

  it("shows the name and icon of a plugin that could not be loaded, with its actions", async () => {
    withPlugins([
      plugin({
        id: "dice",
        name: "Würfel",
        version: "0.4.1",
        status: { kind: "notLoaded", error: "incompatible" },
        icon: "assets/icon.svg",
      }),
    ]);
    vi.spyOn(api, "pluginAsset").mockResolvedValue({
      mimeType: "image/svg+xml",
      base64: "PHN2Zy8+",
    });
    vi.spyOn(api, "pluginUpdates").mockResolvedValue([
      {
        id: "dice",
        name: "Würfel",
        from: "0.4.1",
        to: "0.5.0",
        source: { kind: "official" },
        size: 10,
        approval: null,
        failed: null,
      },
    ]);
    const { container } = render(<InstalledTab />);
    expect(await screen.findByText("Could not be loaded")).toBeInTheDocument();
    expect(container.querySelector(".plugins-installed-name")).toHaveTextContent("Würfel");
    expect(screen.queryByText("dice")).not.toBeInTheDocument();
    await waitFor(() => {
      expect(container.querySelector(".plugins-installed-icon__image")).not.toBeNull();
    });
    expect(
      await screen.findByRole("button", { name: "Update", description: "Würfel" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Remove" })).toBeInTheDocument();
  });

  it("lists the active permissions as chips in a fixed order", () => {
    expect(
      permissionChips(
        plugin({
          required: ["events.voice", "ui.panel"],
          optional: [{ id: "ui.member-badge", suggested: true }],
          granted: ["events.voice", "ui.panel"],
        }),
      ),
    ).toEqual(["uiPanel", "eventsVoice"]);
  });

  it("pins the version and exports the data to a chosen file", async () => {
    withPlugins([echolings]);
    const pin = vi.spyOn(api, "pluginSetPinned").mockResolvedValue(null);
    vi.spyOn(api, "pluginExportData").mockResolvedValue('{"format":1}');
    const saveText = vi.spyOn(api, "saveText").mockResolvedValue(null);
    vi.spyOn(window, "prompt").mockReturnValue("/tmp/echolings.json");
    render(<InstalledTab />);
    fireEvent.click(
      await screen.findByRole("button", { name: "Permissions", description: "Echolings" }),
    );
    fireEvent.click(
      screen.getByRole("checkbox", { name: "Pin version 1.2.0 (no automatic updates)" }),
    );
    await waitFor(() => {
      expect(pin).toHaveBeenCalledWith("echolings", true);
    });
    fireEvent.click(screen.getByRole("button", { name: "Export data" }));
    await waitFor(() => {
      expect(saveText).toHaveBeenCalledWith("/tmp/echolings.json", '{"format":1}');
    });
    expect(await screen.findByText("Data exported.")).toBeInTheDocument();
  });

  it("shows an error when an action fails", async () => {
    withPlugins([echolings]);
    render(<InstalledTab />);
    fireEvent.click(await screen.findByRole("switch", { name: "Echolings on" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("The plugin could not be switched.");
  });

  it("opens the plugin an 'installed' request names", async () => {
    withPlugins([echolings, foo]);
    render(<InstalledTab />);
    await screen.findByText("Official");
    act(() => {
      routePluginRequest({ kind: "installed", plugin: "foo" });
    });
    expect(
      await screen.findByRole("button", { name: "Permissions", description: "foo" }),
    ).toHaveAttribute("aria-expanded", "true");
  });
});
