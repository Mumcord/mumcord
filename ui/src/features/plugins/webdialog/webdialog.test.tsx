import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { applyLanguage } from "../../../i18n";
import { PluginDialogHost } from "../dialog";
import { rememberDialogOpener } from "../dialog/pluginDialogStore";
import * as plugins from "../pluginStore";
import * as store from "../render/pluginUiStore";
import { axeViolations } from "../../../test/axe";

const sendEvent = vi.fn(() => Promise.resolve(null));
vi.mock("../../../api", async (orig) => ({
  ...(await orig<typeof import("../../../api")>()),
  getApi: () => ({
    pluginUiEvent: sendEvent,
    on: () => () => undefined,
    pluginsUiTrees: () => Promise.resolve([]),
  }),
}));
vi.mock("../render/usePluginIcon", () => ({ usePluginIcon: () => null }));
// The frame has its own tests; here it only has to be a focusable page with a way to press Esc.
vi.mock("../webview", () => ({
  PluginFrame: ({ onEscape }: { onEscape?: () => void }) => (
    <div className="plugin-webview" tabIndex={-1}>
      <button type="button" onClick={onEscape}>
        page escape
      </button>
    </div>
  ),
}));

function open(kind: "official" | "repo" = "repo", hint?: { minWidth: number; minHeight: number }) {
  vi.spyOn(store, "usePluginUi").mockReturnValue({
    trees: new Map(),
    dialogs: new Map([["brawl", { title: "Board Brawl", subtitle: null }]]),
    menuEntries: new Map(),
    webviews: new Map([
      ["brawl", new Map([["dialog", { place: "dialog", url: "mumcord-web://x", seq: 1 }]])],
    ]),
    ...(hint ? { sizeHints: new Map([["brawl", hint]]) } : {}),
  });
  vi.spyOn(plugins, "usePlugins").mockReturnValue({
    overview: {
      plugins: [
        {
          id: "brawl",
          name: "Board Brawl",
          source: kind === "official" ? { kind } : { kind, repositoryId: 1, name: "Meeple Works" },
        },
      ],
    },
  } as unknown as ReturnType<typeof plugins.usePlugins>);
}

describe("web dialog", () => {
  beforeAll(async () => {
    await applyLanguage("en");
  });
  afterEach(() => {
    vi.restoreAllMocks();
    sendEvent.mockClear();
  });

  it("draws header, chip, close button and footer in a modal outside any form", () => {
    open();
    const { container } = render(
      <form>
        <PluginDialogHost />
      </form>,
    );
    const dialog = screen.getByRole("dialog", { name: "Board Brawl" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(container.querySelector("form dialog, form [role=dialog]")).toBeNull();
    expect(screen.getByText("Unofficial · Meeple Works")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Close Board Brawl" })).toBeInTheDocument();
    expect(screen.getByText(/Mumcord never asks for passwords there\. Esc closes\./)).toBeVisible();
  });

  it("shows no chip for an official plugin", () => {
    open("official");
    render(<PluginDialogHost />);
    expect(screen.queryByText(/Unofficial/)).toBeNull();
  });

  it("passes the size hint on as the frame's minimum height", () => {
    open("repo", { minWidth: 300, minHeight: 440 });
    render(<PluginDialogHost />);
    expect(screen.getByRole("dialog").style.getPropertyValue("--plugin-dialog-min-height")).toBe(
      "440px",
    );
  });

  it("closes on the host side and sends surface-closed on Esc, on ✕ and on Esc in the page", () => {
    const close = vi.spyOn(store, "closePluginDialog");
    open();
    render(<PluginDialogHost />);
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    fireEvent.click(screen.getByRole("button", { name: "Close Board Brawl" }));
    fireEvent.click(screen.getByRole("button", { name: "page escape" }));
    expect(close).toHaveBeenCalledTimes(3);
    expect(sendEvent).toHaveBeenCalledTimes(3);
    expect(sendEvent).toHaveBeenCalledWith("brawl", { type: "surfaceClosed", place: "dialog" });
  });

  it("starts focus in the page, keeps Tab inside and returns focus to the opener", () => {
    open();
    const opener = document.createElement("button");
    document.body.append(opener);
    rememberDialogOpener("brawl", opener);
    const { unmount } = render(<PluginDialogHost />);
    expect(document.activeElement).toHaveClass("plugin-webview");
    const sentinels = [...document.querySelectorAll<HTMLElement>("[data-focus-sentinel]")];
    expect(sentinels).toHaveLength(2);
    // Tabbing past the end of the page lands on the trailing sentinel, which wraps to the start.
    act(() => sentinels[1]?.focus());
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Close Board Brawl" }));
    // Shift+Tab before the start lands on the leading sentinel, which jumps to the end.
    act(() => sentinels[0]?.focus());
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "page escape" }));
    unmount();
    expect(document.activeElement).toBe(opener);
    opener.remove();
  });

  it("has no accessibility violations", async () => {
    open();
    render(<PluginDialogHost />);
    expect(await axeViolations(document.body)).toEqual([]);
  });
});
