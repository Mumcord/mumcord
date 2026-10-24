import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { announce } from "../../a11y";
import { createMockApi, setApi, type MockApi, type MumbleLink } from "../../api";
import "../../i18n";
import { axeViolations } from "../../test/axe";
import { applyThemeToDocument } from "../../theme/theme";
import { resetServerStore } from "../channels/serverStore";
import { stubDialog } from "../channels/testUtils";
import { ConnectionStatus } from "./ConnectionStatus";
import { initials, splitAddress } from "./format";
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
    <>
      <nav aria-label="Servers">
        <ServersRoot />
      </nav>
      <main>
        <ConnectionStatus />
      </main>
    </>,
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

  it("asks before switching to another server", async () => {
    setup();
    const other = await addServer("Other", "other.example.org");
    await renderAll();
    fireEvent.click(serverButton(0));
    await waitFor(() => {
      expect(serverButtons()[0]).toHaveAttribute("aria-current", "true");
    });
    const connect = vi.spyOn(api, "connect");
    fireEvent.click(serverButton(1));
    expect(dialog()).toHaveTextContent("Disconnect from Local test server and connect to Other?");
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(connect).not.toHaveBeenCalled();

    fireEvent.click(serverButton(1));
    fireEvent.click(screen.getByRole("button", { name: "Disconnect and switch" }));
    expect(connect).toHaveBeenCalledWith(other.id, null);
    await waitFor(() => {
      expect(serverButtons()[1]).toHaveAttribute("aria-current", "true");
    });
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

  it("offers a context menu with Shift+F10; Escape returns focus", async () => {
    setup();
    await addServer("Second", "two.example.org");
    await renderAll();
    const first = serverButton(0);
    first.focus();
    fireEvent.keyDown(first, { key: "F10", shiftKey: true });
    const menu = screen.getByRole("menu", { name: "Options for Local test server" });
    const items = within(menu).getAllByRole("menuitem");
    expect(items.map((i) => i.textContent)).toEqual([
      "Connect",
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

describe("ServerDialog", () => {
  it("adds a server with port 64738 and the identity name by default", async () => {
    setup();
    await renderAll();
    const add = screen.getByRole("button", { name: "Add a server" });
    add.focus();
    fireEvent.click(add);
    expect(dialog()).toHaveAccessibleName("Add a server");
    expect(screen.getByLabelText("Port")).toHaveValue("64738");
    expect(screen.getByLabelText("Your name on this server")).toHaveValue("You");
    fireEvent.change(screen.getByLabelText("Address"), { target: { value: "new.example.org" } });
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
      username: "You",
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
      username: "You",
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
    const password = screen.getByLabelText("Server password (optional)");
    expect(password).toHaveValue("");
    expect(password).toHaveAccessibleDescription(/Leave this empty to keep it/);
    await save();
    expect(update).toHaveBeenLastCalledWith(
      server.id,
      expect.objectContaining({ password: { kind: "keep" } }),
    );

    edit();
    fireEvent.change(screen.getByLabelText("Server password (optional)"), {
      target: { value: "new" },
    });
    await save();
    expect(update).toHaveBeenLastCalledWith(
      server.id,
      expect.objectContaining({ password: { kind: "set", value: "new" } }),
    );

    edit();
    fireEvent.click(screen.getByLabelText("Remove the saved password"));
    expect(screen.getByLabelText("Server password (optional)")).toBeDisabled();
    await save();
    expect(update).toHaveBeenLastCalledWith(
      server.id,
      expect.objectContaining({ password: { kind: "clear" } }),
    );
    expect((await api.serversList())[1]?.hasPassword).toBe(false);
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
    expect(connect).toHaveBeenCalledWith(1, null);
  });

  it("shows TCP fallback", async () => {
    setup();
    await renderAll();
    fireEvent.click(serverButton(0));
    await waitFor(() => {
      expect(serverButtons()[0]).toHaveAttribute("aria-current", "true");
    });
    act(() => {
      api.mock.emit("transportChanged", { transport: "tcp" });
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
    await api.identityCreate("Tobi");
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
    await api.identityCreate("Tobi");
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
      expect(connect).toHaveBeenCalledWith(expect.any(Number), ["Raid"]);
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

  it("connects to a matching saved server with the channel path", async () => {
    setup();
    await renderAll();
    const connect = vi.spyOn(api, "connect");
    act(() => {
      api.mock.emit("deepLinkOpened", { link: link("LOCALHOST", ["Raid"]) });
    });
    expect(connect).toHaveBeenCalledWith(1, ["Raid"]);
  });

  it("asks to switch when another server is connected", async () => {
    setup();
    const other = await addServer("Other", "other.example.org");
    await renderAll();
    fireEvent.click(serverButton(0));
    await waitFor(() => {
      expect(serverButtons()[0]).toHaveAttribute("aria-current", "true");
    });
    act(() => {
      api.mock.emit("deepLinkOpened", { link: link("other.example.org", ["AFK"]) });
    });
    const connect = vi.spyOn(api, "connect");
    fireEvent.click(screen.getByRole("button", { name: "Disconnect and switch" }));
    expect(connect).toHaveBeenCalledWith(other.id, ["AFK"]);
  });

  it("offers to save an unknown server, then connects", async () => {
    setup();
    await renderAll();
    act(() => {
      api.mock.emit("deepLinkOpened", { link: link("new.example.org", ["Lobby"]) });
    });
    expect(dialog()).toHaveAccessibleName("Save this server?");
    expect(screen.getByLabelText("Address")).toHaveValue("new.example.org");
    const connect = vi.spyOn(api, "connect");
    fireEvent.click(screen.getByRole("button", { name: "Save and connect" }));
    await waitFor(() => {
      expect(connect).toHaveBeenCalledWith(2, ["Lobby"]);
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
