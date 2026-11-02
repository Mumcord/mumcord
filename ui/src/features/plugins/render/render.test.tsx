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
import { setSettingsOpener } from "../../settings/settingsOpener";
import { resetPluginRequestStore, usePendingPluginRequest } from "../pluginRequestStore";
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
  component: { type: "stack", direction: "column", gap: 4, align: "start", justify: null },
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
    let s = applyChange(empty, {
      plugin: "p",
      change: { type: "setTree", slot: PANEL, nodes, serverId: null },
    });
    s = applyChange(s, {
      plugin: "p",
      change: { type: "setTree", slot: { kind: "dialog" }, nodes, serverId: null },
    });
    s = applyChange(s, {
      plugin: "p",
      change: { type: "openDialog", title: "Hi", subtitle: "Night Owls" },
    });
    expect(s.trees.get("p")?.get("panel")).toBe(nodes);
    expect(s.dialogs.get("p")).toEqual({ title: "Hi", subtitle: "Night Owls" });
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
        change: { type: "setTree", slot: PANEL, nodes: [stack([])], serverId: null },
      });
    });
    await screen.findByText("1");
  });

  it("opens the plugin's permissions in Installed when it asks to", async () => {
    setSettingsOpener(vi.fn());
    function Probe() {
      const request = usePendingPluginRequest("installed");
      const tree = usePluginTree("poll", PANEL);
      return (
        <span>
          {tree === undefined ? "ready" : "tree"} {request?.plugin ?? "no request"}
        </span>
      );
    }
    render(<Probe />);
    await screen.findByText("ready no request");
    act(() => {
      api.mock.emit("pluginUi", { plugin: "poll", change: { type: "openPermissions" } });
    });
    await screen.findByText("ready poll");
    resetPluginRequestStore();
    setSettingsOpener(null);
  });
});

describe("PluginTree", () => {
  it("renders components with accessible names and no axe violations", async () => {
    const { container } = show([
      stack([1, 2, 3, 4, 5, 6]),
      leaf({ type: "text", text: "Lunch?", style: "heading", color: "accent" }),
      leaf({ type: "meter", label: "Energy", value: 0.5, color: "success" }),
      leaf({ type: "progress", label: "Upload", value: null }),
      leaf({ type: "chip", text: "3 votes", color: null, tone: null }),
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
        detail: null,
      }),
      leaf({
        type: "button",
        label: "Closed",
        action: "b",
        kind: "secondary",
        disabled: false,
        expanded: false,
        detail: null,
      }),
      leaf({
        type: "button",
        label: "Plain",
        action: "c",
        kind: "secondary",
        disabled: false,
        expanded: null,
        detail: null,
      }),
    ]);
    expect(screen.getByRole("button", { name: "Open" })).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("button", { name: "Closed" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    expect(screen.getByRole("button", { name: "Plain" })).not.toHaveAttribute("aria-expanded");
  });

  it("lets the first child of a fill-first row take the free width", () => {
    const row = (justify: "fillFirst" | "start", direction: "row" | "column"): PluginNode => ({
      component: { type: "stack", direction, gap: 8, align: "stretch", justify },
      animation: null,
      children: [1, 2],
    });
    const children = [
      leaf({
        type: "button",
        label: "Rifter",
        action: "open:1",
        kind: "secondary",
        disabled: false,
        expanded: null,
        detail: "Tobias · 20:09",
      }),
      leaf({ type: "copyButton", label: "Copy fitting", text: "[Rifter]", kind: "secondary" }),
    ];
    const { container, unmount } = show([row("fillFirst", "row"), ...children]);
    const stackEl = container.querySelector(".pr-stack");
    expect(stackEl).toHaveClass("pr-stack--row", "pr-stack--fill-first");
    expect(stackEl?.firstElementChild).toBe(screen.getByRole("button", { name: "Rifter" }));
    unmount();

    // Only rows grow a child; a column, or a row with another justify, stays as it was.
    for (const [justify, direction] of [
      ["fillFirst", "column"],
      ["start", "row"],
    ] as const) {
      const shown = show([row(justify, direction), ...children]);
      expect(shown.container.querySelector(".pr-stack")).not.toHaveClass("pr-stack--fill-first");
      shown.unmount();
    }
  });

  it("shows a button's detail line as its description, not its name", async () => {
    const action = vi.spyOn(api, "pluginUiAction").mockResolvedValue(null);
    const { container } = show([
      stack([1, 2]),
      leaf({
        type: "button",
        label: "Rifter",
        action: "open:1",
        kind: "secondary",
        disabled: false,
        expanded: null,
        detail: "Tobias · 20:09",
      }),
      leaf({
        type: "button",
        label: "Plain",
        action: "plain",
        kind: "secondary",
        disabled: false,
        expanded: null,
        detail: null,
      }),
    ]);
    const entry = screen.getByRole("button", { name: "Rifter" });
    expect(entry).toHaveAccessibleDescription("Tobias · 20:09");
    expect(entry).toHaveTextContent("RifterTobias · 20:09");
    expect(screen.getByRole("button", { name: "Plain" })).not.toHaveAttribute("aria-describedby");
    fireEvent.click(entry);
    expect(action).toHaveBeenCalledWith("poll", PANEL, "open:1", { type: "none" }, null);
    expect(await axeViolations(container)).toEqual([]);
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
        detail: null,
      }),
      leaf({ type: "toggle", label: "Notify", value: false, action: "notify", disabled: false }),
    ]);
    fireEvent.click(screen.getByRole("button", { name: "Vote" }));
    fireEvent.click(screen.getByRole("switch", { name: "Notify" }));
    expect(action).toHaveBeenCalledWith("poll", PANEL, "vote", { type: "none" }, null);
    expect(action).toHaveBeenCalledWith(
      "poll",
      PANEL,
      "notify",
      { type: "boolean", value: true },
      null,
    );
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
        detail: null,
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
    const nodes = [leaf({ type: "chip", text: "hi", color: null, tone: null }, "bob")];
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
    expect(action).toHaveBeenCalledWith(
      "poll",
      PANEL,
      "move",
      { type: "reorder", from: 0, to: 1 },
      null,
    );
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
        help: null,
        helpTone: null,
        error: null,
      }),
    ]);
    const input = screen.getByRole("textbox", { name: "Name" });
    fireEvent.change(input, { target: { value: "äääää" } });
    expect(input).toHaveValue("ää");
    expect(screen.getByText("2 / 2 characters")).toBeInTheDocument();
    fireEvent.keyDown(input, { key: "Enter" });
    expect(action).toHaveBeenCalledWith("poll", PANEL, "name", { type: "text", value: "ää" }, null);
  });

  it("sends the password once and clears the field", () => {
    const action = vi.spyOn(api, "pluginUiAction").mockResolvedValue(null);
    show([
      leaf({
        type: "password",
        label: "Token",
        secret: "token",
        action: "saved",
        disabled: false,
        help: null,
        helpTone: null,
        error: null,
      }),
    ]);
    const input = screen.getByLabelText("Token");
    fireEvent.change(input, { target: { value: "s3cret" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(action).toHaveBeenCalledWith(
      "poll",
      PANEL,
      "saved",
      { type: "text", value: "s3cret" },
      null,
    );
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
    expect(action).toHaveBeenCalledWith(
      "poll",
      PANEL,
      "file",
      {
        type: "file",
        path: "/music/song.mp3",
      },
      null,
    );
  });
});

describe("field notes", () => {
  const input = (help: string | null, error: string | null): PluginComponent => ({
    type: "textInput",
    label: "Bot address",
    value: "",
    placeholder: "",
    maxBytes: 64,
    purpose: "serverAddress",
    action: "address",
    disabled: false,
    help,
    helpTone: "success",
    error,
  });

  it("describes the field with its help note in its tone", async () => {
    const { container } = show([leaf(input("Reachable. Bot is online.", null))]);
    const field = screen.getByRole("textbox", { name: "Bot address" });
    expect(field).not.toHaveAttribute("aria-invalid");
    expect(field).toHaveAccessibleDescription(/Reachable\. Bot is online\./);
    expect(screen.getByText("Reachable. Bot is online.")).toHaveClass("pr-note--success");
    expect(await axeViolations(container)).toEqual([]);
  });

  it("shows the error instead of the help and marks the field invalid", async () => {
    const { container } = show([
      stack([1, 2]),
      leaf(input("Reachable.", "Bot not reachable: no answer.")),
      leaf({
        type: "password",
        label: "API key",
        secret: "api-key",
        action: "key",
        disabled: false,
        help: "Not checked yet.",
        helpTone: null,
        error: "Key rejected (401).",
      }),
    ]);
    const field = screen.getByRole("textbox", { name: "Bot address" });
    expect(field).toHaveAttribute("aria-invalid", "true");
    expect(field).toHaveAccessibleDescription(/Bot not reachable: no answer\./);
    expect(screen.queryByText("Reachable.")).not.toBeInTheDocument();
    expect(screen.getByText("Bot not reachable: no answer.")).toHaveClass("pr-note--danger");
    const key = screen.getByLabelText("API key");
    expect(key).toHaveAttribute("aria-invalid", "true");
    expect(key).toHaveAccessibleDescription("Key rejected (401).");
    expect(screen.queryByText("Not checked yet.")).not.toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });
});

describe("variants", () => {
  it("draws button kinds, chip tones and stack justification", () => {
    const button = (label: string, kind: "primary" | "secondary" | "danger"): PluginComponent => ({
      type: "button",
      label,
      action: label,
      kind,
      disabled: false,
      expanded: null,
      detail: null,
    });
    show([
      {
        component: { type: "stack", direction: "row", gap: 8, align: "center", justify: "end" },
        animation: null,
        children: [1, 2, 3, 4, 5],
      },
      leaf(button("Save", "primary")),
      leaf(button("Test connection", "secondary")),
      leaf(button("Remove", "danger")),
      leaf({ type: "chip", text: "Connected", color: "danger", tone: "success" }),
      leaf({ type: "chip", text: "3 votes", color: "accent", tone: null }),
    ]);
    expect(screen.getByRole("button", { name: "Save" })).toHaveClass("button", "button--primary");
    expect(screen.getByRole("button", { name: "Test connection" })).toHaveClass(
      "button",
      "pr-button--secondary",
    );
    expect(screen.getByRole("button", { name: "Remove" })).toHaveClass("pr-button--danger");
    const tone = screen.getByText("Connected");
    expect(tone).toHaveClass("pr-chip", "pr-tone--success");
    expect(tone).not.toHaveAttribute("style");
    expect(screen.getByText("3 votes")).toHaveStyle({ color: "var(--color-accent)" });
    expect(document.querySelector(".pr-stack")).toHaveStyle({ justifyContent: "flex-end" });
  });
});

describe("copy button", () => {
  const copyNode = leaf({
    type: "copyButton",
    label: "Copy fitting",
    text: "[Drake]",
    kind: "secondary",
  });

  it("copies its text on a click and confirms politely", async () => {
    const copyText = vi.fn(() => Promise.resolve(null));
    api.copyText = copyText;
    const action = vi.spyOn(api, "pluginUiAction");
    show([copyNode]);
    const button = screen.getByRole("button", { name: "Copy fitting" });
    expect(button).toHaveClass("pr-button--secondary");
    expect(screen.getByRole("status")).toBeEmptyDOMElement();
    expect(copyText).not.toHaveBeenCalled();
    fireEvent.click(button);
    expect(copyText).toHaveBeenCalledExactlyOnceWith("[Drake]");
    await waitFor(() => {
      expect(screen.getByRole("status")).toHaveTextContent("Copied");
    });
    expect(screen.getByRole("status")).toHaveAttribute("aria-live", "polite");
    // Nothing goes back to the plugin: the click is not a plugin action.
    expect(action).not.toHaveBeenCalled();
  });

  it("says so when the copy fails and forgets the message again", async () => {
    api.copyText = vi.fn(() => Promise.reject(new Error("no clipboard")));
    vi.useFakeTimers({ shouldAdvanceTime: true });
    show([copyNode]);
    fireEvent.click(screen.getByRole("button", { name: "Copy fitting" }));
    await waitFor(() => {
      expect(screen.getByRole("status")).toHaveTextContent("Could not copy");
    });
    act(() => {
      vi.advanceTimersByTime(2500);
    });
    expect(screen.getByRole("status")).toBeEmptyDOMElement();
  });

  it("follows the button kinds", () => {
    show([
      { ...copyNode, component: { ...copyNode.component, kind: "danger" } as PluginComponent },
    ]);
    expect(screen.getByRole("button", { name: "Copy fitting" })).toHaveClass("pr-button--danger");
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

describe("tree updates while typing", () => {
  const addInput: PluginComponent = {
    type: "textInput",
    label: "Add a file name or URL",
    value: "",
    placeholder: "",
    maxBytes: 64,
    purpose: "plain",
    action: "add-text",
    disabled: false,
    help: null,
    helpTone: null,
    error: null,
  };
  const text = (value: string) => leaf({ type: "text", text: value, style: "body", color: null });

  it("shows the plugin's value once it changes, not an older draft", () => {
    // The user types, the plugin takes the text ("abc") and then empties the field after
    // using it; the field must not bring the typed draft back.
    const tree = (value: string) => [leaf({ ...addInput, value })];
    const view = show(tree(""));
    const field = screen.getByRole("textbox", { name: "Add a file name or URL" });
    fireEvent.change(field, { target: { value: "abc" } });
    view.rerender(<PluginTree plugin="poll" slot={PANEL} nodes={tree("abc")} />);
    view.rerender(<PluginTree plugin="poll" slot={PANEL} nodes={tree("")} />);
    expect(screen.getByRole("textbox", { name: "Add a file name or URL" })).toHaveValue("");
  });

  it("shows the plugin's slider value once it changes, not an older position", () => {
    const slider = (value: number) => [
      leaf({
        type: "slider",
        label: "Volume",
        min: 0,
        max: 100,
        step: 5,
        value,
        action: "volume",
        disabled: false,
      }),
    ];
    const view = show(slider(60));
    const range = screen.getByRole("slider", { name: "Volume" });
    fireEvent.change(range, { target: { value: "30" } });
    view.rerender(<PluginTree plugin="poll" slot={PANEL} nodes={slider(30)} />);
    view.rerender(<PluginTree plugin="poll" slot={PANEL} nodes={slider(60)} />);
    expect(screen.getByRole("slider", { name: "Volume" })).toHaveValue("60");
  });

  it("keeps the draft and the focus when an earlier part of the tree grows", () => {
    // Root: [now playing, input]. The now-playing part grows from one node to three, which
    // moves the input from flat index 2 to 4.
    const before = [stack([1, 2]), text("Nothing is playing"), leaf(addInput)];
    const after = [
      stack([1, 4]),
      stack([2, 3]),
      text("Now playing"),
      text("tone-a"),
      leaf(addInput),
    ];
    const view = show(before);
    const field = screen.getByRole("textbox", { name: "Add a file name or URL" });
    field.focus();
    fireEvent.change(field, { target: { value: "http://host/track.mp3" } });
    view.rerender(<PluginTree plugin="poll" slot={PANEL} nodes={after} />);
    const now = screen.getByRole("textbox", { name: "Add a file name or URL" });
    expect(now).toHaveValue("http://host/track.mp3");
    expect(now).toHaveFocus();
  });
});
