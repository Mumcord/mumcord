import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi, type Settings } from "../../api";
import { applyLanguage } from "../../i18n";
import { fakeView } from "../../app/testView";
import { ViewContext } from "../../app/view";
import { AboutSection } from "../settings/sections/AboutSection";
import { PacketInspectorSection } from "../settings/sections/PacketInspectorSection";
import { reloadDevToolsUnlocked, setDevToolsUnlocked, UNLOCK_CLICKS } from "./devTools";
import {
  applyRootLook,
  isCloseShortcut,
  namesOf,
  readRootLook,
  sharedUsers,
} from "./inspectorView";
import { InspectorViewShare } from "./InspectorViewShare";
import { InspectorWindow } from "./InspectorWindow";
import { PacketInspector } from "./PacketInspector";
import { PacketTable } from "./PacketTable";
import type { Names, Packet } from "./packets";

let api: MockApi;
let settings: Settings;

beforeEach(async () => {
  await applyLanguage("en");
  localStorage.clear();
  reloadDevToolsUnlocked();
  api = createMockApi();
  setApi(api);
  settings = await api.settingsGet();
});

afterEach(() => {
  setApi(undefined);
  vi.restoreAllMocks();
});

const names: Names = {
  user: (_s, session) => (session === 2 ? "Alice" : null),
  server: () => "Test server",
};

function packet(over: Partial<Packet>): Packet {
  return {
    id: 1,
    serverId: 1,
    atMs: new Date(2026, 0, 1, 12, 0, 0, 0).getTime(),
    direction: "sent",
    kind: "mumcord.watch",
    pluginData: true,
    size: 18,
    sender: null,
    receivers: [2, 3, 4],
    delayMs: 340,
    tokensLeft: 2,
    decoded: '{\n  "a": "<b>x</b>"\n}',
    rawHex: "7b7d",
    ...over,
  };
}

describe("PacketTable", () => {
  it("shows direction, plain type name, size, receivers and pacer", () => {
    render(<PacketTable packets={[packet({})]} names={names} />);
    const row = screen.getByRole("row", { name: /12:00:00\.000/u });
    expect(within(row).getByText("Sent")).toBeInTheDocument();
    expect(within(row).getByText("Watch party state")).toBeInTheDocument();
    expect(within(row).getByText("mumcord.watch")).toBeInTheDocument();
    expect(within(row).getByText("18 B")).toBeInTheDocument();
    expect(within(row).getByText("→ 3 users")).toBeInTheDocument();
    expect(within(row).getByText("Alice (session 2)")).toBeInTheDocument();
    expect(within(row).getByText(/delayed 340 ms/u)).toBeInTheDocument();
  });

  it("expands a row to the decoded fields, the gap, raw hex and copy", async () => {
    const writeText = vi.spyOn(api, "copyText").mockResolvedValue(null);
    const first = packet({ id: 1 });
    const second = packet({
      id: 2,
      atMs: first.atMs + 340,
      direction: "received",
      sender: 2,
      receivers: [],
      delayMs: null,
      tokensLeft: null,
    });
    render(<PacketTable packets={[first, second]} names={names} />);
    const toggle = screen.getByRole("button", { name: /packet at 12:00:00\.340/u });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("+340 ms")).toBeInTheDocument();
    const code = screen.getByLabelText("Decoded content");
    // Server input stays text: no element is created from it.
    expect(code.querySelector("b")).toBeNull();
    expect(code).toHaveTextContent('"a": "<b>x</b>"');

    fireEvent.click(screen.getByRole("button", { name: "Show raw hex" }));
    expect(screen.getByLabelText("Raw bytes in hex")).toHaveTextContent("0000 7b 7d");

    fireEvent.click(screen.getByRole("button", { name: "Copy packet" }));
    await screen.findByText("Copied.");
    expect(writeText).toHaveBeenCalledWith(expect.stringContaining("← Alice (session 2)"));
  });
});

describe("PacketInspector", () => {
  it("lists captured packets, filters them and exports both formats", async () => {
    setDevToolsUnlocked(true);
    await api.packetCaptureSet(true);
    const save = vi.spyOn(api, "packetCaptureExport");
    const prompt = vi.spyOn(window, "prompt");
    render(<PacketInspector names={names} />);
    expect(await screen.findAllByText("Mumcord hello")).not.toHaveLength(0);
    expect(screen.getByText("Packets (6 of 6)")).toBeInTheDocument();
    expect(screen.getByText(/Voice packets: 1520 sent, 3044 received/u)).toBeInTheDocument();
    // The voice server's line carries the audio rating badge with its reason.
    const voiceLine = screen.getByText(/Voice packets: 1520 sent, 3044 received/u);
    expect(within(voiceLine).getByText("Audio: Fair")).toBeInTheDocument();
    expect(
      within(voiceLine).getByText("Some voice from others was concealed: network loss"),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("combobox", { name: "Direction" }));
    fireEvent.click(screen.getByRole("option", { name: "Received" }));
    expect(screen.getByText("Packets (3 of 6)")).toBeInTheDocument();

    prompt.mockReturnValueOnce("out.json");
    fireEvent.click(screen.getByRole("button", { name: "Export JSON" }));
    await screen.findByText("Export saved.");
    const [jsonPath, json] = save.mock.calls[0] ?? [];
    expect(jsonPath).toBe("out.json");
    expect((JSON.parse(json ?? "") as { packets: unknown[] }).packets).toHaveLength(3);

    prompt.mockReturnValueOnce("out.md");
    fireEvent.click(screen.getByRole("button", { name: "Export Markdown" }));
    await waitFor(() => {
      expect(save).toHaveBeenCalledTimes(2);
    });
    expect(save.mock.calls[1]?.[1]).toContain(
      "| Time | Direction | Type | Size | From / To | Pacer |",
    );
    expect(screen.getByText(/can include personal data/u)).toBeInTheDocument();
  });

  it("clears the list", async () => {
    setDevToolsUnlocked(true);
    await api.packetCaptureSet(true);
    render(<PacketInspector names={names} />);
    await screen.findByText("Packets (6 of 6)");
    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    await screen.findByText("No packets yet.");
  });
});

describe("PacketInspectorSection", () => {
  it("opens the inspector window, once per click", async () => {
    setDevToolsUnlocked(true);
    const open = vi.spyOn(api, "packetInspectorOpen");
    render(<PacketInspectorSection />);
    const button = screen.getByRole("button", { name: "Open packet inspector" });
    fireEvent.click(button);
    fireEvent.click(button);
    expect(open).toHaveBeenCalledTimes(2);
    // The list itself lives in the window, not in Settings.
    expect(screen.queryByText(/^Packets \(/u)).toBeNull();
    await waitFor(() => {
      expect(screen.getByRole("status")).toBeEmptyDOMElement();
    });
  });

  it("reports a window that cannot be opened", async () => {
    setDevToolsUnlocked(true);
    vi.spyOn(api, "packetInspectorOpen").mockRejectedValue(new Error("no display"));
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    render(<PacketInspectorSection />);
    fireEvent.click(screen.getByRole("button", { name: "Open packet inspector" }));
    await waitFor(() => {
      expect(screen.getByRole("status")).not.toBeEmptyDOMElement();
    });
  });

  it("hides the tools again with the switch", () => {
    setDevToolsUnlocked(true);
    render(<PacketInspectorSection />);
    fireEvent.click(screen.getByRole("switch", { name: "Developer tools" }));
    expect(localStorage.getItem("mumcord.devTools")).toBeNull();
  });
});

describe("the inspector window", () => {
  afterEach(() => {
    const root = document.documentElement;
    delete root.dataset.effects;
    root.style.removeProperty("--color-bg");
  });

  it("takes over the main window's look and names, live", async () => {
    await api.packetInspectorShare({
      attributes: { theme: "light", effects: "off" },
      properties: { "--color-bg": "#ffffff" },
      language: "en",
      users: [{ serverId: 1, session: 2, name: "Alice" }],
      servers: [{ serverId: 1, label: "Test server" }],
    });
    render(<InspectorWindow />);
    expect(screen.getByRole("main", { name: "Packet inspector" })).toBeInTheDocument();
    const root = document.documentElement;
    await waitFor(() => {
      expect(root.dataset.theme).toBe("light");
    });
    expect(root.dataset.effects).toBe("off");
    expect(root.style.getPropertyValue("--color-bg")).toBe("#ffffff");

    await act(() =>
      api.packetInspectorShare({
        attributes: { theme: "dark" },
        properties: {},
        language: "en",
        users: [],
        servers: [],
      }),
    );
    expect(root.dataset.theme).toBe("dark");
    expect(root.dataset.effects).toBeUndefined();
    expect(root.style.getPropertyValue("--color-bg")).toBe("");
  });

  it("closes on Ctrl+W but not on Escape", () => {
    const close = vi.spyOn(api, "packetInspectorClose");
    render(<InspectorWindow />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(close).not.toHaveBeenCalled();
    fireEvent.keyDown(window, { key: "w", ctrlKey: true });
    expect(close).toHaveBeenCalledTimes(1);
    expect(isCloseShortcut({ key: "W", ctrlKey: false, metaKey: true })).toBe(true);
    expect(isCloseShortcut({ key: "w", ctrlKey: false, metaKey: false })).toBe(false);
  });
});

describe("sharing the main window's view", () => {
  it("shares the look on start and again only when something changed", async () => {
    const share = vi.spyOn(api, "packetInspectorShare");
    render(<InspectorViewShare />);
    await waitFor(() => {
      expect(share).toHaveBeenCalledTimes(1);
    });
    const root = document.documentElement;
    const shared = () => share.mock.calls.filter(([view]) => view.attributes.inspectorTest === "1");
    act(() => {
      root.dataset.inspectorTest = "1";
    });
    await waitFor(() => {
      expect(shared()).toHaveLength(1);
    });
    const calls = share.mock.calls.length;
    // Writing the same value again changes nothing worth sending.
    await act(async () => {
      root.dataset.inspectorTest = "1";
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(share).toHaveBeenCalledTimes(calls);
    delete root.dataset.inspectorTest;
  });

  it("reads and applies only data attributes and custom properties", () => {
    const from = document.createElement("div");
    from.dataset.theme = "dark";
    from.style.setProperty("--color-bg", "#000000");
    from.style.color = "red";
    from.lang = "de";
    const look = readRootLook(from);
    expect(look).toEqual({
      attributes: { theme: "dark" },
      properties: { "--color-bg": "#000000" },
      language: "de",
    });
    const to = document.createElement("div");
    to.dataset.stale = "1";
    applyRootLook(to, {
      attributes: { ...look.attributes, "bad key": "x" },
      properties: { ...look.properties, color: "blue" },
    });
    expect(Object.fromEntries(Object.entries(to.dataset))).toEqual({ theme: "dark" });
    expect(to.style.getPropertyValue("--color-bg")).toBe("#000000");
    expect(to.style.color).toBe("");
  });

  it("lists the known people in a stable order and looks names up", () => {
    const connected = new Map([
      [2, { users: new Map([[5, { name: "Bob" }]]) }],
      [
        1,
        {
          users: new Map([
            [7, { name: "Carol" }],
            [3, { name: "Alice" }],
          ]),
        },
      ],
    ]);
    const users = sharedUsers(connected);
    expect(users.map((u) => u.name)).toEqual(["Alice", "Carol", "Bob"]);
    const lookup = namesOf({ attributes: {}, properties: {}, language: "en", users, servers: [] });
    expect(lookup.user(1, 7)).toBe("Carol");
    expect(lookup.user(2, 7)).toBeNull();
    expect(namesOf(null).server(1)).toBeNull();
  });
});

describe("unlocking the developer tools", () => {
  it("takes seven clicks on the version in About", async () => {
    render(
      <ViewContext.Provider value={fakeView()}>
        <AboutSection settings={settings} update={vi.fn()} />
      </ViewContext.Provider>,
    );
    const version = await screen.findByText(/^\d+\.\d+\.\d+/u);
    for (let i = 1; i < UNLOCK_CLICKS; i++) fireEvent.click(version);
    expect(screen.queryByText(/Developer tools unlocked/u)).toBeNull();
    act(() => {
      fireEvent.click(version);
    });
    expect(screen.getByRole("status", { name: "" })).toHaveTextContent("Developer tools unlocked");
    expect(localStorage.getItem("mumcord.devTools")).toBe("1");
  });
});
