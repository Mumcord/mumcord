import { fireEvent, render, screen, waitFor } from "@testing-library/react";
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

describe("PluginStrip", () => {
  it("is hidden without a plugin that has a panel", async () => {
    setup([plugin("dice", "Dice", true)], []);
    const { container } = render(<PluginStrip />);
    await new Promise((r) => setTimeout(r, 20));
    expect(container).toBeEmptyDOMElement();
  });

  it("shows one icon per panel plugin and switches the panel", async () => {
    setup(
      [plugin("pets", "Pets"), plugin("poll", "Poll"), plugin("dice", "Dice", false)],
      ["pets", "poll", "dice"],
    );
    render(<PluginStrip />);
    expect(await screen.findByText("pets panel")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: /^(Pets|Poll|Dice)$/ })).toHaveLength(2);
    fireEvent.click(screen.getByRole("button", { name: "Poll" }));
    expect(await screen.findByText("poll panel")).toBeInTheDocument();
  });

  it("moves between icons with the arrow keys", async () => {
    setup([plugin("pets", "Pets"), plugin("poll", "Poll")], ["pets", "poll"]);
    render(<PluginStrip />);
    const first = await screen.findByRole("button", { name: "Pets" });
    first.focus();
    fireEvent.keyDown(first, { key: "ArrowRight" });
    expect(screen.getByRole("button", { name: "Poll" })).toHaveFocus();
  });

  it("opens the popover with Expand and closes it with Escape", async () => {
    setup([plugin("pets", "Pets")], ["pets"]);
    render(<PluginStrip />);
    const expand = await screen.findByRole("button", { name: "Expand Pets" });
    fireEvent.click(expand);
    const dialog = screen.getByRole("dialog", { name: "Pets" });
    expect(dialog).toBeInTheDocument();
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(expand).toHaveFocus();
  });

  it("shows the plugin's web view in the popover instead of the tree", async () => {
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

  it("stays collapsed once the user collapsed it and tells the plugin", async () => {
    setup([plugin("pets", "Pets")], ["pets"]);
    const events = vi.spyOn(api, "pluginUiEvent").mockResolvedValue(null);
    const first = render(<PluginStrip />);
    await screen.findByText("pets panel");
    await waitFor(() => {
      expect(events).toHaveBeenCalledWith("pets", {
        type: "panelVisibility",
        visibility: "inline",
      });
    });
    fireEvent.click(screen.getByRole("button", { name: /Plugins/ }));
    await waitFor(() => {
      expect(events).toHaveBeenLastCalledWith("pets", {
        type: "panelVisibility",
        visibility: "hidden",
      });
    });
    first.unmount();
    render(<PluginStrip />);
    const toggle = await screen.findByRole("button", { name: /Plugins/ });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("pets panel")).toBeNull();
  });

  async function connect() {
    const [server] = await api.serversList();
    await api.connect((server ?? { id: 0 }).id, null, null);
  }

  it("explains greyed shared parts on servers without plugin data", async () => {
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
    api = createMockApi({ seed: "ready" });
    setApi(api);
    resetServerStore();
    setup([plugin("pets", "Pets")], ["pets"]);
    await connect();
    await waitFor(() => { expect(getServerState().serverId).not.toBeNull(); });
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
