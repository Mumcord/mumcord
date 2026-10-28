import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi, type Settings } from "../../api";
import { applyLanguage } from "../../i18n";
import { AboutSection } from "../settings/sections/AboutSection";
import { PacketInspectorSection } from "../settings/sections/PacketInspectorSection";
import { reloadDevToolsUnlocked, setDevToolsUnlocked, UNLOCK_CLICKS } from "./devTools";
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
    const writeText = vi.fn(() => Promise.resolve());
    Object.assign(navigator, { clipboard: { writeText } });
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

describe("PacketInspectorSection", () => {
  it("lists captured packets, filters them and exports both formats", async () => {
    setDevToolsUnlocked(true);
    await api.packetCaptureSet(true);
    const save = vi.spyOn(api, "packetCaptureExport");
    const prompt = vi.spyOn(window, "prompt");
    render(<PacketInspectorSection />);
    expect(await screen.findAllByText("Mumcord hello")).not.toHaveLength(0);
    expect(screen.getByText("Packets (6 of 6)")).toBeInTheDocument();
    expect(screen.getByText(/Voice packets: 1520 sent, 3044 received/u)).toBeInTheDocument();

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

  it("clears the list, and the switch hides the tools again", async () => {
    setDevToolsUnlocked(true);
    await api.packetCaptureSet(true);
    render(<PacketInspectorSection />);
    await screen.findByText("Packets (6 of 6)");
    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    await screen.findByText("No packets yet.");
    fireEvent.click(screen.getByRole("switch", { name: "Developer tools" }));
    expect(localStorage.getItem("mumcord.devTools")).toBeNull();
  });
});

describe("unlocking the developer tools", () => {
  it("takes seven clicks on the version in About", async () => {
    render(<AboutSection settings={settings} update={vi.fn()} />);
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
