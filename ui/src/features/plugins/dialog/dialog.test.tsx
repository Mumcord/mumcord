import { fireEvent, render, screen } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { applyLanguage } from "../../../i18n";
import * as store from "../render/pluginUiStore";
import { PluginDialogHost } from "./PluginDialog";
import { rememberDialogOpener, takeDialogOpener } from "./pluginDialogStore";

const sendEvent = vi.fn(() => Promise.resolve(null));
vi.mock("../../../api", async (orig) => ({
  ...(await orig<typeof import("../../../api")>()),
  getApi: () => ({
    pluginUiEvent: sendEvent,
    on: () => () => undefined,
    pluginsUiTrees: () => Promise.resolve([]),
  }),
}));

function withDialog(title: string | null) {
  vi.spyOn(store, "usePluginUi").mockReturnValue({
    trees: new Map(),
    dialogs: title === null ? new Map() : new Map([["mumbot-music", title]]),
    menuEntries: new Map(),
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

  it("hands the opener back once", () => {
    const button = document.createElement("button");
    rememberDialogOpener("a", button);
    expect(takeDialogOpener("a")).toBe(button);
    expect(takeDialogOpener("a")).toBeUndefined();
  });
});
