import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { announce } from "../../a11y";
import { PreferencesProvider } from "../../app/PreferencesProvider";
import { createMockApi, setApi, type MockApi, type MumbleLink } from "../../api";
import "../../i18n";
import { axeViolations } from "../../test/axe";
import { applyThemeToDocument } from "../../theme/theme";
import { getShownServerId, resetServerStore } from "../channels/serverStore";
import { hideInfo, showInfo, welcomeKey } from "../chat/hiddenInfo";
import { resetFollowStore } from "../follow/followStore";
import { resetMentions } from "./mentions";
import { stubDialog } from "../channels/testUtils";
import { ConnectionStatus } from "./ConnectionStatus";
import { displayName, initials, splitAddress } from "./format";
import { ServersRoot } from "./ServersRoot";

vi.mock("../../a11y", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../a11y")>();
  return { ...original, announce: vi.fn() };
});

let api: MockApi;

function setup(options: Parameters<typeof createMockApi>[0] = {}): MockApi {
  resetServerStore();
  api = createMockApi(options);
  setApi(api);
  return api;
}

async function renderAll() {
  const view = render(
    <PreferencesProvider>
      <nav aria-label="Servers">
        <ServersRoot />
      </nav>
      <main>
        <ConnectionStatus />
      </main>
    </PreferencesProvider>,
  );
  await waitFor(() => {
    expect(screen.queryByRole("button", { name: "Add a server" })).toBeInTheDocument();
  });
  return view;
}

async function addServer(label: string, address: string) {
  const server = await api.serverAdd({
    label,
    address,
    port: 64738,
    username: "You",
    password: { kind: "keep" },
  });
  return server;
}

/** Switches the add/edit dialog to a tab (issue #151). */
const openTab = (name: string) => {
  fireEvent.click(within(dialog()).getByRole("tab", { name }));
};

const serverButtons = () =>
  within(screen.getByRole("list", { name: "Saved servers" })).getAllByRole("button");
function at<T>(items: T[], index: number): T {
  const item = items[index];
  if (item === undefined) throw new Error(`no item at ${String(index)}`);
  return item;
}
const serverButton = (index: number) => at(serverButtons(), index);
const status = () => screen.getByRole("status", { name: "Connection status" });
const dialog = () => screen.getByRole("dialog");

beforeEach(() => {
  stubDialog();
  vi.mocked(announce).mockClear();
});

afterEach(() => {
  vi.useRealTimers();
  resetServerStore();
  resetFollowStore();
  resetMentions();
  setApi(undefined);
});

describe("format", () => {
  it("builds initials and splits addresses", () => {
    expect(initials("Raid night crew")).toBe("RN");
    expect(initials("  ")).toBe("?");
    expect(splitAddress("host:1234")).toEqual({ address: "host", port: 1234 });
    expect(splitAddress("[::1]:5")).toEqual({ address: "::1", port: 5 });
    expect(splitAddress("fe80::1")).toEqual({ address: "fe80::1", port: null });
  });
});

describe("ServerBar", () => {
  it("shows saved servers with initials, a tooltip name and one Tab stop", async () => {
    setup();
    await addServer("Raid Night", "raid.example.org");
    await renderAll();
    const buttons = serverButtons();
    expect(buttons.map((b) => b.textContent)).toEqual(["LT", "RN"]);
    expect(buttons[1]).toHaveAccessibleName("Raid Night");
    expect(buttons.map((b) => b.tabIndex)).toEqual([0, -1]);
    fireEvent.keyDown(at(buttons, 0), { key: "ArrowDown" });
    expect(buttons[1]).toHaveFocus();
    expect(buttons.map((b) => b.tabIndex)).toEqual([-1, 0]);
    fireEvent.keyDown(at(buttons, 1), { key: "Home" });
    expect(buttons[0]).toHaveFocus();
  });

  it("connects on click and marks the connected server", async () => {
    setup();
    await renderAll();
    fireEvent.click(serverButton(0));
    await waitFor(() => {
      expect(serverButtons()[0]).toHaveAttribute("aria-current", "true");
    });
    expect(serverButtons()[0]).toHaveAccessibleName("Local test server (connected)");
    expect(announce).toHaveBeenCalledWith("Connected to Local test server.");
  });

  it("connects another server without asking and shows it, keeping the first (FR-2.6)", async () => {
    setup();
    const other = await addServer("Other", "other.example.org");
    await renderAll();
    fireEvent.click(serverButton(0));
    await waitFor(() => {
      expect(serverButtons()[0]).toHaveAttribute("aria-current", "true");
    });
    const connect = vi.spyOn(api, "connect");
    fireEvent.click(serverButton(1));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(connect).toHaveBeenCalledWith(other.id, null, null);
    await waitFor(() => {
      expect(serverButtons()[1]).toHaveAttribute("aria-current", "true");
    });
    expect((await api.connectionsList()).map((c) => c.serverId)).toEqual([1, other.id]);

    // A connected server is only shown again, not reconnected.
    fireEvent.click(serverButton(0));
    await waitFor(() => {
      expect(serverButtons()[0]).toHaveAttribute("aria-current", "true");
    });
    expect(connect).toHaveBeenCalledTimes(1);
  });

  it("reorders with Alt+Arrow, persists the order and announces it", async () => {
    setup();
    await addServer("Second", "two.example.org");
    await renderAll();
    serverButton(0).focus();
    fireEvent.keyDown(serverButton(0), { key: "ArrowDown", altKey: true });
    expect(serverButtons().map((b) => b.textContent)).toEqual(["S", "LT"]);
    expect(announce).toHaveBeenCalledWith("Moved Local test server to position 2 of 2.");
    await waitFor(async () => {
      expect((await api.serversList()).map((s) => s.label)).toEqual([
        "Second",
        "Local test server",
      ]);
    });
    expect(serverButtons()[1]).toHaveFocus();
  });

  it("reverts the order when saving it fails", async () => {
    setup();
    await addServer("Second", "two.example.org");
    await renderAll();
    api.mock.failNext("serversReorder", "internal");
    fireEvent.keyDown(serverButton(0), { key: "ArrowDown", altKey: true });
    await waitFor(() => {
      expect(serverButtons().map((b) => b.textContent)).toEqual(["LT", "S"]);
    });
    expect(status()).toHaveTextContent("The new server order couldn't be saved.");
  });

  it("reorders by drag and drop", async () => {
    setup();
    await addServer("Second", "two.example.org");
    await renderAll();
    const dataTransfer = { setData: vi.fn(), effectAllowed: "" };
    fireEvent.dragStart(serverButton(1), { dataTransfer });
    fireEvent.dragOver(serverButton(0), { dataTransfer });
    fireEvent.drop(serverButton(0), { dataTransfer });
    expect(serverButtons().map((b) => b.textContent)).toEqual(["S", "LT"]);
  });

  it("offers Show welcome message only while the welcome message is hidden", async () => {
    setup();
    const saved = (await api.serversList())[0];
    if (!saved) throw new Error("no saved server");
    await renderAll();
    const first = serverButton(0);
    fireEvent.contextMenu(first);
    expect(screen.queryByRole("menuitem", { name: "Show welcome message" })).toBeNull();
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    const id = saved.id;
    act(() => {
      hideInfo(welcomeKey(id), "Hello");
    });
    fireEvent.contextMenu(first);
    fireEvent.click(screen.getByRole("menuitem", { name: "Show welcome message" }));
    expect(localStorage.getItem(`mumcord.hidden.${welcomeKey(id)}`)).toBeNull();
    act(() => {
      showInfo(welcomeKey(id));
    });
  });

  it("offers a context menu with Shift+F10; Escape returns focus", async () => {
    setup();
    await addServer("Second", "two.example.org");
    await renderAll();
    const first = serverButton(0);
    first.focus();
    fireEvent.keyDown(first, { key: "F10", shiftKey: true });
    const menu = screen.getByRole("menu", { name: "Options for Local test server" });
    const items = [...menu.querySelectorAll("button")];
    expect(items.map((i) => i.textContent)).toEqual([
      "Connect",
      "Always connect at start",
      "Mute notifications",
      "Mute @all and @channel",
      "Identity…",
      "Theme…",
      "Edit server",
      "Move down",
      "Remove server",
    ]);
    expect(items[0]).toHaveFocus();
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(items[1]).toHaveFocus();
    fireEvent.keyDown(menu, { key: "Escape" });
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(first).toHaveFocus();

    fireEvent.contextMenu(first);
    fireEvent.click(screen.getByRole("menuitem", { name: "Move down" }));
    expect(serverButtons().map((b) => b.textContent)).toEqual(["S", "LT"]);
  });

  it("removes a server after confirmation", async () => {
    setup();
    await addServer("Second", "two.example.org");
    await renderAll();
    fireEvent.contextMenu(serverButton(1));
    fireEvent.click(screen.getByRole("menuitem", { name: "Remove server" }));
    expect(dialog()).toHaveAccessibleName("Remove Second?");
    fireEvent.click(within(dialog()).getByRole("button", { name: "Remove" }));
    await waitFor(() => {
      expect(serverButtons()).toHaveLength(1);
    });
    expect(await api.serversList()).toHaveLength(1);
    expect(announce).toHaveBeenCalledWith("Second removed.");
  });
});

describe("ServerBar with several servers (spec §5.2)", () => {
  async function connectBoth() {
    const other = await addServer("Other", "other.example.org");
    await renderAll();
    fireEvent.click(serverButton(0));
    await waitFor(() => {
      expect(serverButton(0)).toHaveAccessibleName("Local test server (connected)");
    });
    fireEvent.click(serverButton(1));
    await waitFor(() => {
      expect(serverButton(1)).toHaveAttribute("aria-current", "true");
    });
    return other;
  }

  const openMenuOf = (index: number) => {
    fireEvent.contextMenu(serverButton(index));
    return screen.getByRole("menu");
  };

  it("labels connecting, connected, shown and saved servers; the tooltip says the same", async () => {
    setup({ connectDelayMs: 50 });
    await addServer("Other", "other.example.org");
    await renderAll();
    fireEvent.click(serverButton(0));
    await waitFor(() => {
      expect(serverButton(0)).toHaveAccessibleName("Local test server (connecting)");
    });
    expect(serverButton(1)).toHaveAccessibleName("Other");
    await waitFor(() => {
      expect(serverButton(0)).toHaveAccessibleName("Local test server (connected)");
    });
    expect(serverButton(0)).toHaveAttribute("aria-current", "true");
    expect(serverButton(0)).toHaveAttribute("data-tooltip", "Local test server (connected)");
  });

  it("marks the voice server, keeps it while another server is shown", async () => {
    setup();
    await connectBoth();
    expect(serverButton(1)).toHaveAccessibleName("Other (connected)");
    await act(async () => {
      const snapshot = await api.sessionSnapshot(1);
      api.mock.focusServer(1);
      await api.channelJoin(1, snapshot?.channels[0]?.id ?? 0);
    });
    await waitFor(() => {
      expect(serverButton(0)).toHaveAccessibleName("Local test server (connected), voice server");
    });
    expect(serverButton(1)).toHaveAttribute("aria-current", "true");
    expect(serverButton(0)).not.toHaveAttribute("aria-current");
  });

  it("shows unread messages and mentions of a server that is not shown", async () => {
    setup();
    await connectBoth();
    const snapshot = await api.sessionSnapshot(1);
    const self = snapshot?.users.find((u) => u.session === snapshot.selfSession);
    const otherUser = snapshot?.users.find((u) => u.session !== snapshot.selfSession);
    if (!self || !otherUser) throw new Error("mock server needs two people");
    const conversation = { kind: "channel" as const, channelId: self.channelId };
    act(() => {
      api.mock.focusServer(1);
      api.mock.receiveMessage(conversation, "hello", otherUser.session);
    });
    await waitFor(() => {
      expect(serverButton(0)).toHaveAccessibleName(
        "Local test server (connected), unread messages",
      );
    });
    expect(serverButton(0).querySelector(".server-bar__mark--unread")).not.toBeNull();
    act(() => {
      api.mock.receiveMessage(conversation, `hi @${self.name}`, otherUser.session);
    });
    await waitFor(() => {
      expect(serverButton(0)).toHaveAccessibleName("Local test server (connected), mentions: 1");
    });
    expect(serverButton(0).querySelector(".server-bar__mark--mentions")).toHaveTextContent("1");
    await act(() => api.conversationMarkRead(1, conversation));
    await waitFor(() => {
      expect(serverButton(0)).toHaveAccessibleName("Local test server (connected)");
    });
  });

  it("shows a lost connection that gave up and offers Reconnect in the menu", async () => {
    setup();
    await renderAll();
    fireEvent.click(serverButton(0));
    await waitFor(() => {
      expect(serverButton(0)).toHaveAttribute("aria-current", "true");
    });
    act(() => {
      api.mock.loseConnection();
      api.mock.emit("connectionStateChanged", {
        serverId: 1,
        state: { state: "disconnected", error: { code: "serverUnreachable", message: "" } },
      });
    });
    expect(serverButton(0)).toHaveAccessibleName("Local test server (connection lost)");
    const connect = vi.spyOn(api, "connect");
    fireEvent.click(within(openMenuOf(0)).getByRole("menuitem", { name: "Reconnect" }));
    expect(connect).toHaveBeenCalledWith(1, null, null);
  });

  it("disconnects the server whose menu was used, not the shown one (FR-2.9)", async () => {
    setup();
    const other = await connectBoth();
    const disconnect = vi.spyOn(api, "disconnect");
    fireEvent.click(within(openMenuOf(0)).getByRole("menuitem", { name: "Disconnect" }));
    await waitFor(() => {
      expect(serverButton(0)).toHaveAccessibleName("Local test server");
    });
    expect(disconnect).toHaveBeenCalledWith(1);
    expect(getShownServerId()).toBe(other.id);
  });

  it("tells when a server fell back to the default identity", async () => {
    setup();
    await renderAll();
    act(() => {
      api.mock.emit("serverIdentityMissing", { serverId: 1 });
    });
    expect(
      await screen.findByText(
        /connected with the default identity because its own identity is missing/,
      ),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => {
      expect(screen.queryByText(/connected with the default identity/)).not.toBeInTheDocument();
    });
  });

  it("switches Always connect at start and Mute notifications from the menu", async () => {
    setup();
    await renderAll();
    const always = () =>
      within(openMenuOf(0)).getByRole("menuitemcheckbox", { name: "Always connect at start" });
    expect(always()).toHaveAttribute("aria-checked", "false");
    fireEvent.click(always());
    await waitFor(async () => {
      expect((await api.serversList())[0]?.alwaysConnectAtStart).toBe(true);
    });
    expect(always()).toHaveAttribute("aria-checked", "true");
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });

    const mute = () =>
      within(openMenuOf(0)).getByRole("menuitemcheckbox", { name: "Mute notifications" });
    fireEvent.click(mute());
    await waitFor(async () => {
      expect((await api.serversList())[0]?.notificationsMuted).toBe(true);
    });
    expect(mute()).toHaveAttribute("aria-checked", "true");
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });

    const group = () =>
      within(openMenuOf(0)).getByRole("menuitemcheckbox", { name: "Mute @all and @channel" });
    expect(group()).toHaveAttribute("aria-checked", "false");
    fireEvent.click(group());
    await waitFor(async () => {
      expect((await api.serversList())[0]?.groupMentionsMuted).toBe(true);
    });
    expect(group()).toHaveAttribute("aria-checked", "true");
  });

  it("refuses more Always connect servers than the limit, naming the setting (FR-2.7)", async () => {
    setup();
    await api.settingsUpdate({ ...(await api.settingsGet()), maxConnectedServers: 1 });
    await addServer("Other", "other.example.org");
    await api.serverSetAlwaysConnect(1, true);
    await renderAll();
    fireEvent.click(
      within(openMenuOf(1)).getByRole("menuitemcheckbox", { name: "Always connect at start" }),
    );
    await waitFor(() => {
      expect(status()).toHaveTextContent(
        "Only 1 servers can be set to “Always connect at start”, the same as the limit of " +
          "connected servers. Turn it off for another server, or raise the limit in Settings → Servers.",
      );
    });
  });

  it("explains the connected-servers limit when a connect is refused (FR-2.1)", async () => {
    setup();
    await addServer("Other", "other.example.org");
    await renderAll();
    api.mock.failNext("connect", "serverLimitReached");
    fireEvent.click(serverButton(1));
    await waitFor(() => {
      expect(status()).toHaveTextContent(
        "Couldn't connect to Other. You're already connected to the maximum of 5 servers. " +
          "Disconnect a server, or raise the limit in Settings → Servers.",
      );
    });
    expect(serverButton(1)).toHaveAccessibleName("Other (connection failed)");
  });

  it("shows the limit message while another connected server is shown, and connects after raising it (FR-2.1)", async () => {
    setup();
    await addServer("Other", "other.example.org");
    await renderAll();
    fireEvent.click(serverButton(0));
    await waitFor(() => {
      expect(serverButtons()[0]).toHaveAttribute("aria-current", "true");
    });
    api.mock.failNext("connect", "serverLimitReached");
    fireEvent.click(serverButton(1));
    await waitFor(() => {
      expect(status()).toHaveTextContent("Couldn't connect to Other. You're already connected");
    });
    expect(status()).toHaveTextContent("Settings → Servers");
    expect(serverButtons()[0]).toHaveAttribute("aria-current", "true");

    fireEvent.click(serverButton(1));
    await waitFor(() => {
      expect(serverButtons()[1]).toHaveAttribute("aria-current", "true");
    });
    expect((await api.connectionsList()).map((c) => c.serverId)).toHaveLength(2);
  });

  it("picks a theme for the server from its menu and goes back to the app theme", async () => {
    setup();
    await renderAll();
    const [server] = await api.serversList();
    if (!server) throw new Error("no server");
    fireEvent.click(within(openMenuOf(0)).getByRole("menuitem", { name: "Theme…" }));
    const themes = within(dialog()).getByRole("group", { name: "Theme" });
    const circle = (name: string) =>
      within(themes).getByRole("button", { name: new RegExp(`^${name}, theme`) });
    expect(circle("Use app theme")).toHaveAccessibleName("Use app theme, theme, selected");
    fireEvent.click(circle("Deep Ocean"));
    fireEvent.click(within(dialog()).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
    expect((await api.serversList())[0]?.themeId).toBe("preset:ocean");

    fireEvent.click(within(openMenuOf(0)).getByRole("menuitem", { name: "Edit server" }));
    openTab("Behaviour");
    const editThemes = within(dialog()).getByRole("group", { name: "Theme" });
    expect(
      within(editThemes).getByRole("button", { name: "Deep Ocean, theme, selected" }),
    ).toBeInTheDocument();
    fireEvent.click(within(editThemes).getByRole("button", { name: /^Use app theme, theme/ }));
    fireEvent.click(within(dialog()).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
    expect((await api.serversList())[0]?.themeId).toBeNull();
  });
});

describe("ServerDialog", () => {
  it("adds a server with port 64738 and the identity name by default", async () => {
    setup();
    await renderAll();
    const add = screen.getByRole("button", { name: "Add a server" });
    add.focus();
    fireEvent.click(add);
    expect(dialog()).toHaveAccessibleName("Add a server");
    expect(screen.getByLabelText("Port")).toHaveValue("64738");
    expect(screen.getByLabelText("Your name on this server")).toHaveValue("Zoe");
    fireEvent.change(screen.getByLabelText("Address"), { target: { value: "new.example.org" } });
    openTab("Passwords");
    fireEvent.change(screen.getByLabelText("Server password (optional)"), {
      target: { value: "secret" },
    });
    const spy = vi.spyOn(api, "serverAdd");
    fireEvent.click(within(dialog()).getByRole("button", { name: "Add server" }));
    await waitFor(() => {
      expect(serverButtons()).toHaveLength(2);
    });
    expect(spy).toHaveBeenCalledWith({
      label: "new.example.org",
      address: "new.example.org",
      port: 64738,
      username: "Zoe",
      password: { kind: "set", value: "secret" },
    });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(add).toHaveFocus();
  });

  it("shows inline errors and focuses the field", async () => {
    setup();
    await renderAll();
    fireEvent.click(screen.getByRole("button", { name: "Add a server" }));
    fireEvent.change(screen.getByLabelText("Port"), { target: { value: "99999" } });
    fireEvent.click(within(dialog()).getByRole("button", { name: "Add server" }));
    const address = screen.getByLabelText("Address");
    expect(address).toHaveAttribute("aria-invalid", "true");
    expect(address).toHaveAccessibleDescription(/Enter the server address/);
    expect(address).toHaveFocus();
    fireEvent.change(address, { target: { value: "x.example.org" } });
    fireEvent.click(within(dialog()).getByRole("button", { name: "Add server" }));
    expect(screen.getByLabelText("Port")).toHaveFocus();
    expect(screen.getByLabelText("Port")).toHaveAccessibleDescription(/between 1 and 65535/);
  });

  it("groups the fields in four tabs and starts on Connection", async () => {
    setup();
    await renderAll();
    fireEvent.click(screen.getByRole("button", { name: "Add a server" }));
    const tabs = within(dialog()).getAllByRole("tab");
    expect(tabs.map((tab) => tab.textContent)).toEqual([
      "Connection",
      "Identity",
      "Passwords",
      "Behaviour",
    ]);
    expect(within(dialog()).getByRole("tab", { name: "Connection" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(within(dialog()).getByRole("tabpanel")).toHaveAccessibleName("Connection");
    expect(screen.getByLabelText("Address")).toBeInTheDocument();
    expect(screen.queryByLabelText("Identity for this server")).not.toBeInTheDocument();
  });

  it("switches tabs with the arrow keys, Home and End", async () => {
    setup();
    await renderAll();
    fireEvent.click(screen.getByRole("button", { name: "Add a server" }));
    const connection = within(dialog()).getByRole("tab", { name: "Connection" });
    connection.focus();
    fireEvent.keyDown(connection, { key: "ArrowRight" });
    expect(within(dialog()).getByRole("tab", { name: "Identity" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    fireEvent.keyDown(within(dialog()).getByRole("tab", { name: "Identity" }), { key: "End" });
    expect(within(dialog()).getByRole("tab", { name: "Behaviour" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    fireEvent.keyDown(within(dialog()).getByRole("tab", { name: "Behaviour" }), {
      key: "ArrowRight",
    });
    expect(within(dialog()).getByRole("tab", { name: "Connection" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    fireEvent.keyDown(within(dialog()).getByRole("tab", { name: "Connection" }), {
      key: "ArrowLeft",
    });
    expect(within(dialog()).getByRole("tab", { name: "Behaviour" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    fireEvent.keyDown(within(dialog()).getByRole("tab", { name: "Behaviour" }), { key: "Home" });
    expect(within(dialog()).getByRole("tab", { name: "Connection" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("marks the tab that needs attention", async () => {
    setup();
    await renderAll();
    fireEvent.click(screen.getByRole("button", { name: "Add a server" }));
    fireEvent.change(screen.getByLabelText("Address"), { target: { value: "a b" } });
    fireEvent.click(within(dialog()).getByRole("button", { name: "Add server" }));
    const connection = within(dialog()).getByRole("tab", { name: /Connection/ });
    expect(connection).toHaveAccessibleName(/needs attention/);
    expect(within(dialog()).getByRole("tab", { name: "Identity" })).not.toHaveTextContent("!");
  });

  it("jumps to the Connection tab when another tab was open on submit", async () => {
    setup();
    await renderAll();
    fireEvent.click(screen.getByRole("button", { name: "Add a server" }));
    openTab("Behaviour");
    fireEvent.click(within(dialog()).getByRole("button", { name: "Add server" }));
    expect(within(dialog()).getByRole("tab", { name: /Connection/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await waitFor(() => {
      expect(screen.getByLabelText("Address")).toHaveFocus();
    });
  });

  it("remembers the last tab of an edit and keeps the identity dropdown there", async () => {
    sessionStorage.clear();
    setup();
    await renderAll();
    fireEvent.contextMenu(serverButton(0));
    fireEvent.click(screen.getByRole("menuitem", { name: "Edit server" }));
    openTab("Identity");
    expect(screen.getByLabelText("Identity for this server")).toBeInTheDocument();
    fireEvent.click(within(dialog()).getByRole("button", { name: "Cancel" }));
    fireEvent.contextMenu(serverButton(0));
    fireEvent.click(screen.getByRole("menuitem", { name: "Edit server" }));
    expect(within(dialog()).getByRole("tab", { name: "Identity" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    sessionStorage.clear();
  });

  it("saves Always connect at start and the theme for a new server", async () => {
    setup();
    await renderAll();
    fireEvent.click(screen.getByRole("button", { name: "Add a server" }));
    fireEvent.change(screen.getByLabelText("Address"), { target: { value: "new.example.org" } });
    openTab("Behaviour");
    fireEvent.click(within(dialog()).getByRole("checkbox", { name: "Always connect at start" }));
    fireEvent.click(
      within(within(dialog()).getByRole("group", { name: "Theme" })).getByRole("button", {
        name: /^Deep Ocean, theme/,
      }),
    );
    fireEvent.click(within(dialog()).getByRole("button", { name: "Add server" }));
    await waitFor(async () => {
      expect((await api.serversList())[1]?.alwaysConnectAtStart).toBe(true);
    });
    expect((await api.serversList())[1]?.themeId).toBe("preset:ocean");
  });

  it("tests the connection with the values in the form", async () => {
    setup();
    await renderAll();
    fireEvent.click(screen.getByRole("button", { name: "Add a server" }));
    fireEvent.change(screen.getByLabelText("Address"), { target: { value: "localhost" } });
    const spy = vi.spyOn(api, "serverTestConnection");
    fireEvent.click(screen.getByRole("button", { name: "Test connection" }));
    const result = await screen.findByText(/Ping: 12 ms · Server version: 1.5.0/);
    expect(result).toHaveTextContent("localhost");
    expect(
      screen.getByText("Certificate not trusted yet – you'll be asked when you connect"),
    ).toBeInTheDocument();
    expect(spy).toHaveBeenCalledWith({
      address: "localhost",
      port: 64738,
      username: "Zoe",
      password: { kind: "keep" },
      serverId: null,
      identityId: null,
    });
    // Nothing was saved.
    expect(await api.serversList()).toHaveLength(1);
    expect(screen.getByRole("dialog")).toHaveAccessibleName("Add a server");
  });

  it("shows a busy test and then the refusal of the server", async () => {
    setup();
    await renderAll();
    fireEvent.click(screen.getByRole("button", { name: "Add a server" }));
    fireEvent.change(screen.getByLabelText("Address"), {
      target: { value: "unreachable.example.org" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Test connection" }));
    expect(await screen.findByText(/Couldn't reach the server/)).toBeInTheDocument();
    expect(screen.getByText(/unreachable\.example\.org/)).toBeInTheDocument();
  });

  it("keeps the test button disabled while a test runs", async () => {
    setup();
    await renderAll();
    fireEvent.click(screen.getByRole("button", { name: "Add a server" }));
    fireEvent.change(screen.getByLabelText("Address"), { target: { value: "localhost" } });
    let release: (() => void) | undefined;
    vi.spyOn(api, "serverTestConnection").mockImplementation(
      () =>
        new Promise((resolve) => {
          release = () => {
            resolve({
              pingMs: 5,
              version: "1.5.0",
              fingerprint: "AA",
              certificate: "trusted",
            });
          };
        }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Test connection" }));
    expect(screen.getByRole("button", { name: "Testing…" })).toBeDisabled();
    expect(screen.getByText("Testing the connection…")).toBeInTheDocument();
    act(() => {
      release?.();
    });
    expect(await screen.findByText(/Ping: 5 ms/)).toBeInTheDocument();
    expect(screen.getByText("Certificate already trusted")).toBeInTheDocument();
  });

  it("reports a wrong password from the test", async () => {
    setup();
    await renderAll();
    fireEvent.click(screen.getByRole("button", { name: "Add a server" }));
    fireEvent.change(screen.getByLabelText("Address"), { target: { value: "localhost" } });
    openTab("Passwords");
    fireEvent.change(screen.getByLabelText("Server password (optional)"), {
      target: { value: "wrong" },
    });
    openTab("Connection");
    fireEvent.click(screen.getByRole("button", { name: "Test connection" }));
    expect(await screen.findByText(/The server refused the password/)).toBeInTheDocument();
  });

  it("fills the fields from a pasted mumble:// link", async () => {
    setup();
    await renderAll();
    fireEvent.click(screen.getByRole("button", { name: "Add a server" }));
    fireEvent.change(screen.getByLabelText("Address"), {
      target: { value: "mumble://Anna@voice.example.org:1234/Raid" },
    });
    await waitFor(() => {
      expect(screen.getByLabelText("Address")).toHaveValue("voice.example.org");
    });
    expect(screen.getByLabelText("Port")).toHaveValue("1234");
    expect(screen.getByLabelText("Your name on this server")).toHaveValue("Anna");
    expect(screen.getByText("Filled in from the mumble:// link.")).toBeInTheDocument();
  });

  it("keeps, replaces or removes the saved password", async () => {
    setup();
    const server = await api.serverAdd({
      label: "Locked",
      address: "locked.example.org",
      port: 64738,
      username: "Zoe",
      password: { kind: "set", value: "old" },
    });
    await renderAll();
    const update = vi.spyOn(api, "serverUpdate");
    const edit = () => {
      fireEvent.contextMenu(serverButton(1));
      fireEvent.click(screen.getByRole("menuitem", { name: "Edit server" }));
      expect(dialog()).toHaveAccessibleName("Edit server");
    };
    const save = async () => {
      fireEvent.click(within(dialog()).getByRole("button", { name: "Save" }));
      await waitFor(() => {
        expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      });
    };

    edit();
    openTab("Passwords");
    const password = screen.getByLabelText("Server password (optional)");
    expect(password).toHaveValue("");
    expect(password).toHaveAccessibleDescription(/Leave this empty to keep it/);
    await save();
    expect(update).toHaveBeenLastCalledWith(
      server.id,
      expect.objectContaining({ password: { kind: "keep" } }),
    );

    edit();
    openTab("Passwords");
    fireEvent.change(screen.getByLabelText("Server password (optional)"), {
      target: { value: "new" },
    });
    await save();
    expect(update).toHaveBeenLastCalledWith(
      server.id,
      expect.objectContaining({ password: { kind: "set", value: "new" } }),
    );

    edit();
    openTab("Passwords");
    fireEvent.click(screen.getByLabelText("Remove the saved password"));
    expect(screen.getByLabelText("Server password (optional)")).toBeDisabled();
    await save();
    expect(update).toHaveBeenLastCalledWith(
      server.id,
      expect.objectContaining({ password: { kind: "clear" } }),
    );
    expect((await api.serversList())[1]?.hasPassword).toBe(false);
  });

  it("saves Always connect at start in the edit dialog (FR-2.7)", async () => {
    setup();
    await renderAll();
    fireEvent.contextMenu(serverButton(0));
    fireEvent.click(screen.getByRole("menuitem", { name: "Edit server" }));
    openTab("Behaviour");
    const check = within(dialog()).getByRole("checkbox", { name: "Always connect at start" });
    expect(check).not.toBeChecked();
    fireEvent.click(check);
    fireEvent.click(within(dialog()).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
    expect((await api.serversList())[0]?.alwaysConnectAtStart).toBe(true);
  });

  it("keeps the edit dialog open when the Always connect limit is reached", async () => {
    setup();
    await api.settingsUpdate({ ...(await api.settingsGet()), maxConnectedServers: 1 });
    await addServer("Other", "other.example.org");
    await api.serverSetAlwaysConnect(1, true);
    await renderAll();
    fireEvent.contextMenu(serverButton(1));
    fireEvent.click(screen.getByRole("menuitem", { name: "Edit server" }));
    openTab("Behaviour");
    fireEvent.click(within(dialog()).getByRole("checkbox", { name: "Always connect at start" }));
    fireEvent.click(within(dialog()).getByRole("button", { name: "Save" }));
    expect(await within(dialog()).findByRole("alert")).toHaveTextContent(
      /^Only 1 servers can be set to “Always connect at start”/,
    );
  });

  it("closes on Escape (cancel event)", async () => {
    setup();
    await renderAll();
    fireEvent.click(screen.getByRole("button", { name: "Add a server" }));
    fireEvent(dialog(), new Event("cancel", { cancelable: true }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});

describe("certificate trust", () => {
  it("asks for an unknown certificate and connects when trusted", async () => {
    setup();
    api.mock.requireCertTrust(1);
    await renderAll();
    fireEvent.click(serverButton(0));
    expect(dialog()).toHaveAccessibleName("Trust this server?");
    expect(dialog()).toHaveTextContent(/Certificate fingerprint/);
    const decide = vi.spyOn(api, "certTrustDecide");
    fireEvent.click(screen.getByRole("button", { name: "Trust and connect" }));
    expect(decide).toHaveBeenCalledWith(1, expect.any(String), true);
    await waitFor(() => {
      expect(serverButtons()[0]).toHaveAttribute("aria-current", "true");
    });
  });

  it("warns about a changed certificate; Escape means don't trust", async () => {
    setup();
    api.mock.requireCertTrust(1, "AA:BB");
    const view = await renderAll();
    fireEvent.click(serverButton(0));
    const warning = screen.getByRole("alertdialog");
    expect(warning).toHaveAccessibleName("Warning: the server's certificate changed");
    expect(warning).toHaveTextContent("AA:BB");
    expect(screen.getByRole("button", { name: "Don't connect" })).toHaveFocus();
    expect(await axeViolations(view.container)).toEqual([]);
    const decide = vi.spyOn(api, "certTrustDecide");
    fireEvent(warning, new Event("cancel", { cancelable: true }));
    expect(decide).toHaveBeenCalledWith(1, expect.any(String), false);
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    await waitFor(() => {
      expect(status()).toHaveTextContent("Couldn't connect to Local test server.");
    });
  });

  it("keeps one server's trust question when another server's connection ends", async () => {
    setup();
    const other = await addServer("Other", "other.example.org");
    await renderAll();
    act(() => {
      api.mock.emit("certTrustRequired", {
        serverId: other.id,
        fingerprint: "CC:DD",
        previousFingerprint: null,
      });
      api.mock.emit("connectionStateChanged", {
        serverId: 1,
        state: { state: "connecting", serverId: 1 },
      });
      api.mock.emit("connectionStateChanged", {
        serverId: 1,
        state: { state: "disconnected", error: { code: "wrongPassword", message: "" } },
      });
    });
    expect(screen.getByRole("dialog", { name: "Trust this server?" })).toHaveTextContent("CC:DD");
  });
});

describe("ConnectionStatus", () => {
  it("is empty while idle and shows Connecting with Cancel", async () => {
    setup({ connectDelayMs: 50 });
    await renderAll();
    expect(status()).toBeEmptyDOMElement();
    fireEvent.click(serverButton(0));
    expect(status()).toHaveTextContent("Connecting to Local test server…");
    fireEvent.click(within(status()).getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(status()).toBeEmptyDOMElement();
    });
  });

  it("counts down while reconnecting and cancels", async () => {
    setup();
    await renderAll();
    fireEvent.click(serverButton(0));
    await waitFor(() => {
      expect(serverButtons()[0]).toHaveAttribute("aria-current", "true");
    });
    vi.useFakeTimers();
    act(() => {
      api.mock.loseConnection();
    });
    expect(announce).toHaveBeenCalledWith(
      "Connection to Local test server lost. Reconnecting…",
      "assertive",
    );
    expect(status()).toHaveTextContent("attempt 1 of 10, next try in 2 s");
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(within(status()).getByText(/next try in 1 s/)).toHaveAttribute("aria-hidden", "true");
    vi.useRealTimers();
    const cancel = vi.spyOn(api, "reconnectCancel");
    fireEvent.click(within(status()).getByRole("button", { name: "Cancel" }));
    expect(cancel).toHaveBeenCalled();
    await waitFor(() => {
      expect(status()).toHaveTextContent("Disconnected");
    });
    expect(within(status()).getByRole("button", { name: "Reconnect" })).toBeInTheDocument();
  });

  it("explains a gave-up connection loss and reconnects", async () => {
    setup();
    await renderAll();
    fireEvent.click(serverButton(0));
    await waitFor(() => {
      expect(serverButtons()[0]).toHaveAttribute("aria-current", "true");
    });
    act(() => {
      api.mock.loseConnection();
      api.mock.emit("connectionStateChanged", {
        serverId: 1,
        state: { state: "disconnected", error: { code: "serverUnreachable", message: "" } },
      });
    });
    expect(status()).toHaveTextContent("Disconnected – The server can't be reached.");
    expect(announce).toHaveBeenCalledWith(
      "Disconnected from Local test server. See the bar above the chat for what to do next.",
      "assertive",
    );
    const connect = vi.spyOn(api, "connect");
    fireEvent.click(within(status()).getByRole("button", { name: "Reconnect" }));
    expect(connect).toHaveBeenCalledWith(1, null, null);
  });

  it("shows TCP fallback", async () => {
    setup();
    await renderAll();
    fireEvent.click(serverButton(0));
    await waitFor(() => {
      expect(serverButtons()[0]).toHaveAttribute("aria-current", "true");
    });
    act(() => {
      api.mock.emit("transportChanged", { serverId: 1, transport: "tcp" });
    });
    expect(status()).toHaveTextContent("Connected via TCP (UDP blocked).");
    expect(announce).toHaveBeenCalledWith(expect.stringContaining("TCP"));
  });

  it("offers Edit server for a wrong password and Try again", async () => {
    setup();
    await renderAll();
    api.mock.failNext("connect", "wrongPassword");
    fireEvent.click(serverButton(0));
    await waitFor(() => {
      expect(status()).toHaveTextContent("The server password is wrong.");
    });
    expect(within(status()).getByRole("button", { name: "Try again" })).toBeInTheDocument();
    fireEvent.click(within(status()).getByRole("button", { name: "Edit server" }));
    expect(dialog()).toHaveAccessibleName("Edit server");
  });

  it("shows nothing after a requested disconnect", async () => {
    setup();
    await renderAll();
    fireEvent.click(serverButton(0));
    await waitFor(() => {
      expect(serverButtons()[0]).toHaveAttribute("aria-current", "true");
    });
    fireEvent.contextMenu(serverButton(0));
    fireEvent.click(screen.getByRole("menuitem", { name: "Disconnect" }));
    await waitFor(() => {
      expect(serverButtons()[0]).not.toHaveAttribute("aria-current");
    });
    expect(status()).toBeEmptyDOMElement();
    expect(announce).toHaveBeenCalledWith("Disconnected from Local test server.");
  });
});

describe("empty state", () => {
  it("adds and connects from an address", async () => {
    setup({ seed: "fresh" });
    await api.identityCreate("Tobi", null);
    await renderAll();
    expect(screen.getByRole("heading", { name: "Add your first server" })).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Server address or mumble:// link"), {
      target: { value: "voice.example.org:1000" },
    });
    const connect = vi.spyOn(api, "connect");
    fireEvent.click(screen.getByRole("button", { name: "Add and connect" }));
    await waitFor(() => {
      expect(connect).toHaveBeenCalled();
    });
    const [saved] = await api.serversList();
    expect(saved).toMatchObject({ address: "voice.example.org", port: 1000, username: "Tobi" });
  });

  it("pastes a mumble:// link from the clipboard, with a fallback hint", async () => {
    setup({ seed: "fresh" });
    await api.identityCreate("Tobi", null);
    const readText = vi.fn().mockRejectedValueOnce(new Error("denied"));
    Object.defineProperty(navigator, "clipboard", { value: { readText }, configurable: true });
    const view = await renderAll();
    expect(await axeViolations(view.container)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Paste a mumble:// link" }));
    expect(await screen.findByText(/press Ctrl\+V/)).toBeInTheDocument();

    readText.mockResolvedValueOnce("mumble://link.example.org/Raid");
    const connect = vi.spyOn(api, "connect");
    fireEvent.click(screen.getByRole("button", { name: "Paste a mumble:// link" }));
    await waitFor(() => {
      expect(connect).toHaveBeenCalledWith(expect.any(Number), ["Raid"], null);
    });
  });
});

describe("deep links", () => {
  const link = (address: string, channelPath: string[] = []): MumbleLink => ({
    address,
    port: 64738,
    username: null,
    password: null,
    channelPath,
  });

  it("shows the server the engine connected for a link", async () => {
    setup();
    const other = await addServer("Other", "other.example.org");
    await renderAll();
    const connect = vi.spyOn(api, "connect");
    act(() => {
      api.mock.emit("deepLinkOpened", {
        link: link("other.example.org", ["AFK"]),
        outcome: { kind: "connecting", serverId: other.id },
      });
    });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(getShownServerId()).toBe(other.id);
    expect(connect).not.toHaveBeenCalled();
  });

  it("offers to save an unknown server, then connects", async () => {
    setup();
    await renderAll();
    act(() => {
      api.mock.emit("deepLinkOpened", {
        link: link("new.example.org", ["Lobby"]),
        outcome: { kind: "unknownServer" },
      });
    });
    expect(dialog()).toHaveAccessibleName("Save this server?");
    expect(screen.getByLabelText("Address")).toHaveValue("new.example.org");
    const connect = vi.spyOn(api, "connect");
    fireEvent.click(screen.getByRole("button", { name: "Save and connect" }));
    await waitFor(() => {
      expect(connect).toHaveBeenCalledWith(2, ["Lobby"], null);
    });
  });
});

describe("accessibility", () => {
  it.each(["light", "dark"] as const)("has no axe violations (%s)", async (theme) => {
    applyThemeToDocument(theme);
    setup();
    await addServer("Second", "two.example.org");
    const view = await renderAll();
    fireEvent.click(serverButton(0));
    await waitFor(() => {
      expect(serverButtons()[0]).toHaveAttribute("aria-current", "true");
    });
    act(() => {
      api.mock.loseConnection();
    });
    expect(await axeViolations(view.container)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Add a server" }));
    expect(await axeViolations(view.container)).toEqual([]);
  });
});

describe("failed-connection banner", () => {
  async function failConnect() {
    api.mock.failNext("connect", "serverUnreachable");
    fireEvent.click(serverButton(0));
    await waitFor(() => {
      expect(status()).toHaveTextContent("Couldn't connect to");
    });
  }

  afterEach(() => {
    vi.useRealTimers();
  });

  it("hides after 60 s but the server keeps its error state", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    setup();
    await renderAll();
    await failConnect();
    act(() => {
      vi.advanceTimersByTime(59_000);
    });
    expect(status()).toHaveTextContent("Couldn't connect to");
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(status()).toBeEmptyDOMElement();
    expect(serverButton(0)).toHaveAccessibleName("Local test server (connection failed)");
  });

  it("dismisses with a labelled button", async () => {
    setup();
    await renderAll();
    await failConnect();
    fireEvent.click(within(status()).getByRole("button", { name: "Dismiss" }));
    expect(status()).toBeEmptyDOMElement();
  });

  it("pauses the timer while hovered or focused", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    setup();
    await renderAll();
    await failConnect();
    const bar = status().querySelector(".servers-status") as HTMLElement;
    fireEvent.mouseEnter(bar);
    act(() => {
      vi.advanceTimersByTime(120_000);
    });
    expect(status()).toHaveTextContent("Couldn't connect to");
    fireEvent.mouseLeave(bar);
    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(status()).toBeEmptyDOMElement();
    // Keyboard focus pauses it too.
    await failConnect();
    const again = status().querySelector(".servers-status") as HTMLElement;
    fireEvent.focus(again);
    act(() => {
      vi.advanceTimersByTime(120_000);
    });
    expect(status()).toHaveTextContent("Couldn't connect to");
  });

  it("shows again on a new failure, not for the same one", async () => {
    setup();
    await renderAll();
    await failConnect();
    fireEvent.click(within(status()).getByRole("button", { name: "Dismiss" }));
    expect(status()).toBeEmptyDOMElement();
    await failConnect();
    expect(status()).toHaveTextContent("Couldn't connect to");
  });

  it("names a server without a label by host and port, never by id", () => {
    expect(displayName({ label: "Raid", address: "a.example.org", port: 1 })).toBe("Raid");
    expect(displayName({ label: "  ", address: "a.example.org", port: 64738 })).toBe(
      "a.example.org:64738",
    );
  });
});
