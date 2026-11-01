import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createMockApi,
  setApi,
  type MockApi,
  type PluginComponent,
  type PluginNode,
  type PluginSlot,
} from "../../../api";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { byteLength, charCount, clampBytes, dropIndex, formatRemaining } from "./helpers";
import { applyChange, slotKey, usePluginTree, type PluginUiState } from "./pluginUiStore";
import { PluginTree } from "./PluginTree";
import { TOKEN_VARS, tokenColor } from "./tokens";

const PANEL: PluginSlot = { kind: "panel" };

const leaf = (component: PluginComponent, animation: PluginNode["animation"] = null) => ({
  component,
  animation,
  children: [],
});

const stack = (children: number[]): PluginNode => ({
  component: { type: "stack", direction: "column", gap: 4, align: "start" },
  animation: null,
  children,
});

let api: MockApi;
let reduced = false;

beforeEach(async () => {
  await applyLanguage("en");
  api = createMockApi();
  setApi(api);
  reduced = false;
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: query.includes("reduced-motion") && reduced,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  }));
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
  setApi(undefined);
});

function show(nodes: PluginNode[]) {
  return render(<PluginTree plugin="poll" slot={PANEL} nodes={nodes} />);
}

describe("theme tokens", () => {
  it("covers exactly the Rust token list", () => {
    const rust = readFileSync(
      resolve(import.meta.dirname, "../../../../../crates/plugins/src/api/theme.rs"),
      "utf8",
    );
    const list = /TOKENS: &\[&str\] = &\[([^\]]*)\]/.exec(rust)?.[1] ?? "";
    const tokens = [...list.matchAll(/"([^"]+)"/g)].map((m) => m[1]);
    expect(Object.keys(TOKEN_VARS).sort()).toEqual(tokens.sort());
  });

  it("maps tokens to CSS variables and ignores anything else", () => {
    expect(tokenColor("accent")).toBe("var(--color-accent)");
    expect(tokenColor("#ff0000")).toBeUndefined();
    expect(tokenColor("constructor")).toBeUndefined();
    expect(tokenColor(null)).toBeUndefined();
  });
});

describe("plugin UI store", () => {
  const empty: PluginUiState = {
    trees: new Map(),
    dialogs: new Map(),
    menuEntries: new Map(),
    webviews: new Map(),
  };

  it("sets, clears and closes with the dialog", () => {
    const nodes = [stack([])];
    let s = applyChange(empty, { plugin: "p", change: { type: "setTree", slot: PANEL, nodes } });
    s = applyChange(s, {
      plugin: "p",
      change: { type: "setTree", slot: { kind: "dialog" }, nodes },
    });
    s = applyChange(s, { plugin: "p", change: { type: "openDialog", title: "Hi" } });
    expect(s.trees.get("p")?.get("panel")).toBe(nodes);
    expect(s.dialogs.get("p")).toBe("Hi");
    s = applyChange(s, { plugin: "p", change: { type: "closeDialog" } });
    expect(s.dialogs.has("p")).toBe(false);
    expect(s.trees.get("p")?.has("dialog")).toBe(false);
    s = applyChange(s, { plugin: "p", change: { type: "clear", slot: PANEL } });
    expect(s.trees.get("p")?.size).toBe(0);
    s = applyChange(s, {
      plugin: "p",
      change: { type: "userMenuEntry", label: "Pet", sessions: null },
    });
    expect(s.menuEntries.get("p")).toEqual({ label: "Pet", sessions: null });
    s = applyChange(s, {
      plugin: "p",
      change: { type: "userMenuEntry", label: "Pet", sessions: [4] },
    });
    expect(s.menuEntries.get("p")).toEqual({ label: "Pet", sessions: [4] });
    s = applyChange(s, {
      plugin: "p",
      change: { type: "userMenuEntry", label: null, sessions: null },
    });
    expect(s.menuEntries.size).toBe(0);
  });

  it("keys slots by kind and target", () => {
    expect(slotKey({ kind: "memberBadge", session: 7 })).toBe("memberBadge:7");
    expect(slotKey({ kind: "chatCard", id: "a" })).toBe("chatCard:a");
    expect(slotKey({ kind: "settings" })).toBe("settings");
  });

  it("follows plugin://ui events", async () => {
    function Probe() {
      const tree = usePluginTree("poll", PANEL);
      return <span>{tree === undefined ? "none" : String(tree.length)}</span>;
    }
    render(<Probe />);
    await screen.findByText("none");
    act(() => {
      api.mock.emit("pluginUi", {
        plugin: "poll",
        change: { type: "setTree", slot: PANEL, nodes: [stack([])] },
      });
    });
    await screen.findByText("1");
  });
});

describe("PluginTree", () => {
  it("renders components with accessible names and no axe violations", async () => {
    const { container } = show([
      stack([1, 2, 3, 4, 5, 6]),
      leaf({ type: "text", text: "Lunch?", style: "heading", color: "accent" }),
      leaf({ type: "meter", label: "Energy", value: 0.5, color: "success" }),
      leaf({ type: "progress", label: "Upload", value: null }),
      leaf({ type: "chip", text: "3 votes", color: null }),
      leaf({ type: "toggle", label: "Notify", value: true, action: "notify", disabled: false }),
      leaf({
        type: "select",
        label: "Song",
        options: [{ value: "a", label: "A" }],
        selected: "a",
        action: "song",
        refreshAction: "reload",
        disabled: false,
      }),
    ]);
    expect(screen.getByRole("heading", { name: "Lunch?" })).toHaveStyle({
      color: "var(--color-accent)",
    });
    expect(screen.getByRole("meter", { name: "Energy" })).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Notify" })).toBeChecked();
    expect(screen.getByRole("button", { name: "Refresh Song" })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("announces the open or closed state of a disclosure button", () => {
    show([
      stack([1, 2, 3]),
      leaf({
        type: "button",
        label: "Open",
        action: "a",
        kind: "secondary",
        disabled: false,
        expanded: true,
      }),
      leaf({
        type: "button",
        label: "Closed",
        action: "b",
        kind: "secondary",
        disabled: false,
        expanded: false,
      }),
      leaf({
        type: "button",
        label: "Plain",
        action: "c",
        kind: "secondary",
        disabled: false,
        expanded: null,
      }),
    ]);
    expect(screen.getByRole("button", { name: "Open" })).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("button", { name: "Closed" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    expect(screen.getByRole("button", { name: "Plain" })).not.toHaveAttribute("aria-expanded");
  });

  it("sends button, toggle and refresh actions for its slot", () => {
    const action = vi.spyOn(api, "pluginUiAction").mockResolvedValue(null);
    show([
      stack([1, 2]),
      leaf({
        type: "button",
        label: "Vote",
        action: "vote",
        kind: "primary",
        disabled: false,
        expanded: null,
      }),
      leaf({ type: "toggle", label: "Notify", value: false, action: "notify", disabled: false }),
    ]);
    fireEvent.click(screen.getByRole("button", { name: "Vote" }));
    fireEvent.click(screen.getByRole("switch", { name: "Notify" }));
    expect(action).toHaveBeenCalledWith("poll", PANEL, "vote", { type: "none" });
    expect(action).toHaveBeenCalledWith("poll", PANEL, "notify", { type: "boolean", value: true });
  });

  it("tells the user when an action fails", async () => {
    show([
      leaf({
        type: "button",
        label: "Vote",
        action: "vote",
        kind: "primary",
        disabled: false,
        expanded: null,
      }),
    ]);
    fireEvent.click(screen.getByRole("button", { name: "Vote" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("could not handle");
  });

  it("survives broken trees", () => {
    show([stack([0, 1, 9]), stack([0])]);
    expect(document.querySelectorAll(".pr-stack")).toHaveLength(2);
    const { container } = show([]);
    expect(container).toHaveTextContent("shows nothing here yet");
  });

  it("animates only without reduced motion", () => {
    const nodes = [leaf({ type: "chip", text: "hi", color: null }, "bob")];
    const { container, unmount } = show(nodes);
    expect(container.querySelector(".pr-anim--bob")).not.toBeNull();
    unmount();
    reduced = true;
    expect(show(nodes).container.querySelector(".pr-anim")).toBeNull();
  });
});

describe("icon button", () => {
  it("draws the icon as a mask in the text colour with the label as its name", async () => {
    vi.spyOn(api, "pluginAsset").mockResolvedValue({ mimeType: "image/svg+xml", base64: "AA==" });
    vi.stubGlobal("URL", { ...URL, createObjectURL: () => "blob:icon", revokeObjectURL: vi.fn() });
    const { container } = show([
      leaf({
        type: "iconButton",
        icon: "assets/icons/help.svg",
        label: "Help",
        action: "help",
        disabled: false,
      }),
    ]);
    expect(screen.getByRole("button", { name: "Help" })).toBeInTheDocument();
    await waitFor(() => {
      expect(container.querySelector(".pr-icon-mask")).toHaveStyle({
        maskImage: 'url("blob:icon")',
      });
    });
    expect(container.querySelector("img")).toBeNull();
  });
});

describe("sprite", () => {
  const sprite = leaf({
    type: "sprite",
    asset: "assets/pet.png",
    alt: "Pet",
    frameWidth: 10,
    frameHeight: 10,
    frames: 4,
    fps: 30,
    stillFrame: 2,
  });

  beforeEach(() => {
    vi.spyOn(api, "pluginAsset").mockResolvedValue({ mimeType: "image/png", base64: "AA==" });
    vi.stubGlobal("URL", { ...URL, createObjectURL: () => "blob:x", revokeObjectURL: vi.fn() });
  });

  it("plays at most 12 frames per second", async () => {
    vi.useFakeTimers();
    show([sprite]);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    const img = screen.getByRole("img", { name: "Pet" });
    // 1000 ms at 12 fps is 12 ticks, i.e. frame 12 % 4 = 0, not 30 % 4.
    expect(img.dataset.frame).toBe("0");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000 / 12);
    });
    expect(img.dataset.frame).toBe("1");
  });

  it("shows the still frame with reduced motion", async () => {
    reduced = true;
    show([sprite]);
    const img = await screen.findByRole("img", { name: "Pet" });
    expect(img.dataset.frame).toBe("2");
  });
});

describe("list", () => {
  const rows = (reorderAction: string | null): PluginNode[] => [
    {
      component: { type: "list", label: "Queue", reorderAction },
      animation: null,
      children: [1, 2, 3],
    },
    leaf({ type: "text", text: "one", style: "body", color: null }),
    leaf({ type: "text", text: "two", style: "body", color: null }),
    leaf({ type: "text", text: "three", style: "body", color: null }),
  ];

  it("reorders with the keyboard and announces it", async () => {
    const action = vi.spyOn(api, "pluginUiAction").mockResolvedValue(null);
    show(rows("move"));
    fireEvent.keyDown(screen.getByRole("button", { name: /Move row 1 of 3/ }), {
      key: "ArrowDown",
    });
    expect(action).toHaveBeenCalledWith("poll", PANEL, "move", { type: "reorder", from: 0, to: 1 });
    expect(screen.getByRole("status")).toHaveTextContent("Moved row 1 to position 2 of 3");
    fireEvent.keyDown(screen.getByRole("button", { name: /Move row 1 of 3/ }), { key: "ArrowUp" });
    expect(action).toHaveBeenCalledTimes(1);
    await waitFor(() => {
      expect(screen.getByRole("list", { name: "Queue" }).children).toHaveLength(3);
    });
  });

  it("has no handles without a reorder action", () => {
    show(rows(null));
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("finds the drop row from the pointer position", () => {
    expect(dropIndex([10, 30, 50], 5)).toBe(0);
    expect(dropIndex([10, 30, 50], 35)).toBe(2);
    expect(dropIndex([10, 30, 50], 99)).toBe(2);
  });
});

describe("inputs", () => {
  it("counts bytes and never splits a character", () => {
    expect(byteLength("äb")).toBe(3);
    expect(clampBytes("aää", 4)).toBe("aä");
  });

  it("counts characters and how many still fit", () => {
    expect(charCount("", 64)).toEqual({ used: 0, max: 64 });
    expect(charCount("abc", 64)).toEqual({ used: 3, max: 64 });
    expect(charCount("ää", 4)).toEqual({ used: 2, max: 2 });
    expect(charCount("ä", 4)).toEqual({ used: 1, max: 3 });
  });

  it("limits the text input to its bytes and sends on Enter", () => {
    const action = vi.spyOn(api, "pluginUiAction").mockResolvedValue(null);
    show([
      leaf({
        type: "textInput",
        label: "Name",
        value: "",
        placeholder: "",
        maxBytes: 4,
        purpose: "plain",
        action: "name",
        disabled: false,
      }),
    ]);
    const input = screen.getByRole("textbox", { name: "Name" });
    fireEvent.change(input, { target: { value: "äääää" } });
    expect(input).toHaveValue("ää");
    expect(screen.getByText("2 / 2 characters")).toBeInTheDocument();
    fireEvent.keyDown(input, { key: "Enter" });
    expect(action).toHaveBeenCalledWith("poll", PANEL, "name", { type: "text", value: "ää" });
  });

  it("sends the password once and clears the field", () => {
    const action = vi.spyOn(api, "pluginUiAction").mockResolvedValue(null);
    show([
      leaf({ type: "password", label: "Token", secret: "token", action: "saved", disabled: false }),
    ]);
    const input = screen.getByLabelText("Token");
    fireEvent.change(input, { target: { value: "s3cret" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(action).toHaveBeenCalledWith("poll", PANEL, "saved", { type: "text", value: "s3cret" });
    expect(input).toHaveValue("");
    expect(screen.getByRole("status")).toHaveTextContent("Saved securely");
  });

  it("sends the picked file path", async () => {
    const action = vi.spyOn(api, "pluginUiAction").mockResolvedValue(null);
    vi.stubGlobal("prompt", () => "/music/song.mp3");
    show([
      leaf({
        type: "filePicker",
        label: "Song",
        accept: [".mp3"],
        action: "file",
        disabled: false,
      }),
    ]);
    fireEvent.click(screen.getByRole("button", { name: "Song: Choose file…" }));
    expect(await screen.findByText("song.mp3")).toBeInTheDocument();
    expect(action).toHaveBeenCalledWith("poll", PANEL, "file", {
      type: "file",
      path: "/music/song.mp3",
    });
  });
});

describe("countdown", () => {
  it("formats the remaining time", () => {
    expect(formatRemaining(9)).toBe("0:09");
    expect(formatRemaining(245)).toBe("4:05");
    expect(formatRemaining(3723)).toBe("1:02:03");
    expect(formatRemaining(-5)).toBe("0:00");
  });

  it("shows when the time is up", () => {
    show([leaf({ type: "countdown", label: "Poll ends", until: 0 })]);
    expect(screen.getByRole("timer")).toHaveTextContent("Time's up");
  });
});
