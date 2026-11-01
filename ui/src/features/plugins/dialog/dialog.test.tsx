import { fireEvent, render, renderHook, screen } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { applyLanguage } from "../../../i18n";
import * as store from "../render/pluginUiStore";
import { PluginDialogHost } from "./PluginDialog";
import { rememberDialogOpener, takeDialogOpener } from "./pluginDialogStore";
import { usePluginMenuEntries } from "./usePluginMenuEntries";

const sendEvent = vi.fn(() => Promise.resolve(null));
vi.mock("../../../api", async (orig) => ({
  ...(await orig<typeof import("../../../api")>()),
  getApi: () => ({
    pluginUiEvent: sendEvent,
    on: () => () => undefined,
    pluginsUiTrees: () => Promise.resolve([]),
  }),
}));

vi.mock("../render/usePluginIcon", () => ({
  usePluginIcon: () => "data:image/svg+xml,icon",
}));

function withDialog(title: string | null, subtitle: string | null = null) {
  vi.spyOn(store, "usePluginUi").mockReturnValue({
    trees: new Map(),
    dialogs: title === null ? new Map() : new Map([["mumbot-music", { title, subtitle }]]),
    menuEntries: new Map(),
    webviews: new Map(),
  });
}

describe("PluginDialogHost", () => {
  beforeAll(async () => {
    await applyLanguage("en");
  });

  it("shows nothing without an open dialog", () => {
    withDialog(null);
    const { container } = render(<PluginDialogHost />);
    expect(container).toBeEmptyDOMElement();
  });

  it("draws the header: plugin icon, title, subtitle and a close button", () => {
    withDialog("Configure bot", "Mumbot · Night Owls Guild");
    const { container } = render(<PluginDialogHost />);
    expect(screen.getByRole("dialog", { name: "Configure bot" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Configure bot" })).toBeInTheDocument();
    expect(screen.getByText("Mumbot · Night Owls Guild")).toBeInTheDocument();
    expect(container.ownerDocument.querySelector(".userpanel-modal__head img")).toHaveAttribute(
      "alt",
      "",
    );
    expect(screen.getAllByRole("button")).toHaveLength(1);
  });

  it("sends dialog-closed on Escape and on Close", () => {
    withDialog("Configure bot");
    render(<PluginDialogHost />);
    expect(screen.getByRole("dialog", { name: "Configure bot" })).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(sendEvent).toHaveBeenCalledWith("mumbot-music", { type: "dialogClosed" });
    sendEvent.mockClear();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(sendEvent).toHaveBeenCalledTimes(1);
  });

  it("closes the dialog in the store at once, even if the plugin never answers", () => {
    const close = vi.spyOn(store, "closePluginDialog");
    withDialog("Configure bot");
    render(<PluginDialogHost />);
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(close).toHaveBeenCalledWith("mumbot-music");
  });

  it("drops the dialog and its web view from the state; a later close is a no-op", () => {
    const open = { type: "openDialog", title: "T", subtitle: null } as const;
    let s = store.applyChange(
      { trees: new Map(), dialogs: new Map(), menuEntries: new Map(), webviews: new Map() },
      { plugin: "p", change: open },
    );
    s = store.applyChange(s, {
      plugin: "p",
      change: { type: "webviewShow", place: "dialog", url: "u" },
    });
    const once = store.applyChange(s, { plugin: "p", change: { type: "closeDialog" } });
    expect(once.dialogs.size).toBe(0);
    expect(once.webviews.size).toBe(0);
    const twice = store.applyChange(once, { plugin: "p", change: { type: "closeDialog" } });
    expect(twice.dialogs.size).toBe(0);
  });

  it("hands the opener back once", () => {
    const button = document.createElement("button");
    rememberDialogOpener("a", button);
    expect(takeDialogOpener("a")).toBe(button);
    expect(takeDialogOpener("a")).toBeUndefined();
  });
});

describe("usePluginMenuEntries", () => {
  it("shows a filtered entry only on the listed users", () => {
    vi.spyOn(store, "usePluginUi").mockReturnValue({
      trees: new Map(),
      dialogs: new Map(),
      menuEntries: new Map([
        ["pets", { label: "Pet", sessions: null }],
        ["mumbot-music", { label: "Configure bot", sessions: [7] }],
      ]),
      webviews: new Map(),
    });
    const labels = (session: number | undefined) =>
      renderHook(() => usePluginMenuEntries(session)).result.current.map((e) => e.label);
    expect(labels(7)).toEqual(["Pet", "Configure bot"]);
    expect(labels(8)).toEqual(["Pet"]);
    expect(labels(undefined)).toEqual([]);
  });
});
