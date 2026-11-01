import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type InstalledPlugin, type MockApi } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { getServerState, resetServerStore } from "../../channels/serverStore";
import { resetPluginStore } from "../pluginStore";
import { PluginStrip } from "./PluginStrip";

function plugin(id: string, name: string, enabled = true): InstalledPlugin {
  return {
    id,
    name,
    version: "1.0.0",
    source: { kind: "official" },
    status: { kind: "running" },
    enabled,
    disabledReason: null,
    required: [],
    optional: [],
    granted: [],
    visibility: "everyone",
    pinned: false,
    notifications: true,
    installedAt: null,
    badge: false,
    revocation: null,
    icon: null,
    description: "",
    authors: [],
  };
}

const textTree = (text: string) => [
  {
    component: { type: "text" as const, text, style: "body" as const, color: null },
    animation: null,
    children: [],
  },
];

const webviewMock = vi.hoisted(() => ({ open: false }));
vi.mock("../webview", () => ({
  usePluginWebView: (_id: string, place: string) =>
    webviewMock.open && place === "panelPopover"
      ? { place, url: "mumcord-web://x/index.html", seq: 1 }
      : undefined,
  PluginWebView: ({ plugin }: { plugin: string }) => <div data-testid="wv">{plugin}</div>,
}));

let api: MockApi;

function setup(plugins: InstalledPlugin[], withPanel: string[]) {
  api.pluginsList = () => Promise.resolve({ noPlugins: false, allOff: false, plugins });
  api.pluginsUiTrees = () =>
    Promise.resolve(
      withPanel.map((id) => ({
        plugin: id,
        change: {
          type: "setTree" as const,
          slot: { kind: "panel" as const },
          nodes: textTree(`${id} panel`),
        },
      })),
    );
}

beforeEach(async () => {
  await applyLanguage("en");
  resetPluginStore();
  window.localStorage.clear();
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
});

const open = (id: string) => {
  window.localStorage.setItem("mumcord.pluginStrip.open", id);
};

describe("PluginStrip", () => {
  it("is hidden without a plugin that has a panel", async () => {
    setup([plugin("dice", "Dice", true)], []);
    const { container } = render(<PluginStrip />);
    await new Promise((r) => setTimeout(r, 20));
    expect(container).toBeEmptyDOMElement();
  });

  it("shows only icons, no header card, and toggles the mini view per icon", async () => {
    setup(
      [plugin("pets", "Pets"), plugin("poll", "Poll"), plugin("dice", "Dice", false)],
      ["pets", "poll", "dice"],
    );
    render(<PluginStrip />);
    const pets = await screen.findByRole("button", { name: "Pets" });
    expect(screen.getAllByRole("button", { name: /^(Pets|Poll|Dice)$/ })).toHaveLength(2);
    expect(screen.queryByText(/running|aktiv/)).toBeNull();
    expect(screen.queryByText("pets panel")).toBeNull();
    expect(pets).toHaveAttribute("aria-pressed", "false");
    expect(pets).toHaveAttribute("data-tooltip", "Pets");
    expect(pets).not.toHaveAttribute("title");
    fireEvent.click(pets);
    expect(await screen.findByText("pets panel")).toBeInTheDocument();
    expect(pets).toHaveAttribute("aria-pressed", "true");
    expect(pets.getAttribute("aria-controls")).toBe(
      screen.getByText("pets panel").closest("[id]")?.id,
    );
    fireEvent.click(screen.getByRole("button", { name: "Poll" }));
    expect(await screen.findByText("poll panel")).toBeInTheDocument();
    expect(screen.queryByText("pets panel")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Poll" }));
    expect(screen.queryByText("poll panel")).toBeNull();
  });

  it("moves between icons with the arrow keys", async () => {
    setup([plugin("pets", "Pets"), plugin("poll", "Poll")], ["pets", "poll"]);
    render(<PluginStrip />);
    const first = await screen.findByRole("button", { name: "Pets" });
    expect(screen.getByRole("toolbar")).toBeInTheDocument();
    first.focus();
    fireEvent.keyDown(first, { key: "ArrowRight" });
    const poll = screen.getByRole("button", { name: "Poll" });
    expect(poll).toHaveFocus();
    expect(poll).toHaveAttribute("aria-pressed", "false");
    fireEvent.keyDown(poll, { key: "Home" });
    expect(first).toHaveFocus();
    fireEvent.keyDown(first, { key: "End" });
    expect(poll).toHaveFocus();
  });

  it("opens the popover with Expand and closes it with Escape", async () => {
    open("pets");
    setup([plugin("pets", "Pets")], ["pets"]);
    render(<PluginStrip />);
    const expand = await screen.findByRole("button", { name: "Expand Pets" });
    fireEvent.click(expand);
    const dialog = screen.getByRole("dialog", { name: "Pets" });
    expect(dialog).toBeInTheDocument();
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("button", { name: "Expand Pets" })).toHaveFocus();
  });

  it("hides the inline area completely while the sheet is open and restores it after", async () => {
    open("pets");
    setup([plugin("pets", "Pets")], ["pets"]);
    const { container } = render(<PluginStrip />);
    fireEvent.click(await screen.findByRole("button", { name: "Expand Pets" }));
    expect(container.querySelector(".pstrip-panel")).toBeNull();
    expect(screen.queryByRole("button", { name: "Expand Pets" })).toBeNull();
    fireEvent.keyDown(screen.getByRole("dialog", { name: "Pets" }), { key: "Escape" });
    expect(container.querySelector(".pstrip-panel")).not.toBeNull();
  });

  it("opens and closes the popover when the plugin asks", async () => {
    setup([plugin("pets", "Pets"), plugin("poll", "Poll")], ["pets", "poll"]);
    render(<PluginStrip />);
    await screen.findByRole("button", { name: "Poll" });
    act(() => {
      api.mock.emit("pluginUi", { plugin: "poll", change: { type: "panelPopover", open: true } });
    });
    expect(await screen.findByRole("dialog", { name: "Poll" })).toBeInTheDocument();
    act(() => {
      api.mock.emit("pluginUi", { plugin: "poll", change: { type: "panelPopover", open: false } });
    });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("shows the plugin's web view in the popover instead of the tree", async () => {
    open("pets");
    setup([plugin("pets", "Pets")], ["pets"]);
    render(<PluginStrip />);
    fireEvent.click(await screen.findByRole("button", { name: "Expand Pets" }));
    expect(screen.queryByTestId("wv")).toBeNull();
    expect(screen.getByText("pets panel")).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole("dialog", { name: "Pets" }), { key: "Escape" });
    webviewMock.open = true;
    try {
      fireEvent.click(screen.getByRole("button", { name: "Expand Pets" }));
      expect(screen.getByTestId("wv")).toHaveTextContent("pets");
      expect(screen.queryByText("pets panel")).toBeNull();
    } finally {
      webviewMock.open = false;
    }
  });

  it("remembers the open plugin, tells it about visibility and closes on a second click", async () => {
    setup([plugin("pets", "Pets")], ["pets"]);
    const events = vi.spyOn(api, "pluginUiEvent").mockResolvedValue(null);
    const first = render(<PluginStrip />);
    fireEvent.click(await screen.findByRole("button", { name: "Pets" }));
    await waitFor(() => {
      expect(events).toHaveBeenCalledWith("pets", {
        type: "panelVisibility",
        visibility: "inline",
      });
    });
    first.unmount();
    const second = render(<PluginStrip />);
    expect(await screen.findByText("pets panel")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Pets" }));
    await waitFor(() => {
      expect(events).toHaveBeenLastCalledWith("pets", {
        type: "panelVisibility",
        visibility: "hidden",
      });
    });
    second.unmount();
    render(<PluginStrip />);
    await screen.findByRole("button", { name: "Pets" });
    expect(screen.queryByText("pets panel")).toBeNull();
  });

  it("reports expanded when a plugin opens its sheet and hidden after it closes", async () => {
    setup([plugin("pets", "Pets"), plugin("poll", "Poll")], ["pets", "poll"]);
    const events = vi.spyOn(api, "pluginUiEvent").mockResolvedValue(null);
    render(<PluginStrip />);
    await screen.findByRole("button", { name: "Poll" });
    act(() => {
      api.mock.emit("pluginUi", { plugin: "poll", change: { type: "panelPopover", open: true } });
    });
    await waitFor(() => {
      expect(events).toHaveBeenCalledWith("poll", {
        type: "panelVisibility",
        visibility: "expanded",
      });
    });
    fireEvent.keyDown(screen.getByRole("dialog", { name: "Poll" }), { key: "Escape" });
    expect(screen.getByRole("button", { name: "Expand Poll" })).toHaveFocus();
  });

  async function connect() {
    const [server] = await api.serversList();
    await api.connect((server ?? { id: 0 }).id, null, null);
  }

  it("explains greyed shared parts on servers without plugin data", async () => {
    open("pets");
    api = createMockApi({ seed: "ready", serverVersion: "1.3.0" });
    setApi(api);
    resetServerStore();
    setup([plugin("pets", "Pets")], ["pets"]);
    await connect();
    const { container } = render(<PluginStrip />);
    expect(
      await screen.findByText("This server doesn't support shared plugin features"),
    ).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("shows no hint on a server with plugin data and follows the engine's decision", async () => {
    open("pets");
    api = createMockApi({ seed: "ready" });
    setApi(api);
    resetServerStore();
    setup([plugin("pets", "Pets")], ["pets"]);
    await connect();
    await waitFor(() => {
      expect(getServerState().serverId).not.toBeNull();
    });
    const serverId = getServerState().serverId;
    render(<PluginStrip />);
    await screen.findByText("pets panel");
    expect(screen.queryByText("This server doesn't support shared plugin features")).toBeNull();
    api.mock.emit("pluginsSharedState", { serverId: serverId ?? 0, supported: false });
    expect(
      await screen.findByText("This server doesn't support shared plugin features"),
    ).toBeInTheDocument();
  });
});
