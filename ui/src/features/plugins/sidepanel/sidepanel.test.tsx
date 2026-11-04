import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import axe from "axe-core";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { applyLanguage } from "../../../i18n";
import { requestPluginScreen } from "../notifications/pluginRequests";
import { SidePanelHost } from "./SidePanelHost";
import { clampSideWidth, minWidthForHint, resetSidePanelDismissals } from "./sidePanelStore";

const api = vi.hoisted(() => ({
  sendEvent: vi.fn<(plugin: string, event: unknown) => Promise<null>>(() => Promise.resolve(null)),
  listeners: new Map<string, (payload: unknown) => void>(),
  widths: new Map<string, number>(),
}));
vi.mock("../notifications/pluginRequests", () => ({ requestPluginScreen: vi.fn() }));
vi.mock("../../../api", async (orig) => {
  const mocked = {
    pluginUiEvent: api.sendEvent,
    pluginSidePanelWidth: (plugin: string) => Promise.resolve(api.widths.get(plugin) ?? null),
    pluginSetSidePanelWidth: (plugin: string, width: number) => {
      api.widths.set(plugin, width);
      return Promise.resolve(null);
    },
    pluginFramesMute: () => Promise.resolve(null),
    pluginsList: () =>
      Promise.resolve({
        noPlugins: false,
        allOff: false,
        plugins: [
          { id: "sketch", name: "Sketchboard", icon: null, source: { kind: "repo" } },
          { id: "dice", name: "Dice", icon: null, source: { kind: "official" } },
        ],
      }),
    on: (name: string, handler: (payload: unknown) => void) => {
      const key = `${name}:${String(Math.random())}`;
      api.listeners.set(key, handler);
      return () => {
        api.listeners.delete(key);
      };
    },
    pluginsUiTrees: () => Promise.resolve([]),
  };
  return { ...(await orig<typeof import("../../../api")>()), getApi: () => mocked };
});
vi.mock("../../channels/serverStore", () => ({
  useVoiceServerId: () => 1,
  useServerStateOf: () => ({}),
  selfUser: () => ({ selfDeaf: false, serverDeaf: false }),
}));

function emit(name: string, payload: unknown) {
  act(() => {
    for (const [key, handler] of api.listeners) if (key.startsWith(`${name}:`)) handler(payload);
  });
}

const show = (plugin: string) => {
  emit("pluginUi", {
    plugin,
    change: { type: "webviewShow", place: "sidePanel", url: `mumcord-web://localhost/${plugin}/` },
  });
};

function mount() {
  return render(
    <div className="shell">
      <aside className="shell__members">
        <SidePanelHost>
          <button type="button">Members</button>
        </SidePanelHost>
      </aside>
    </div>,
  );
}

beforeAll(async () => {
  await applyLanguage("en");
});
beforeEach(() => {
  api.sendEvent.mockClear();
  api.widths.clear();
  vi.mocked(requestPluginScreen).mockClear();
  resetSidePanelDismissals();
});
afterEach(() => {
  emit("pluginUi", { plugin: "sketch", change: { type: "webviewHide", place: "sidePanel" } });
  emit("pluginUi", { plugin: "dice", change: { type: "webviewHide", place: "sidePanel" } });
});

describe("width", () => {
  it("keeps 18–32 rem and follows the size hint", () => {
    expect(clampSideWidth(5)).toBe(18);
    expect(clampSideWidth(99)).toBe(32);
    expect(minWidthForHint(undefined)).toBe(18);
    expect(minWidthForHint(400)).toBe(25);
    expect(minWidthForHint(9000)).toBe(32);
  });

  it("opens at the width the engine remembers for the plugin", async () => {
    api.widths.set("sketch", 27);
    const { container } = mount();
    show("sketch");
    await screen.findByRole("region", { name: "Sketchboard side panel" });
    const shell = container.querySelector<HTMLElement>(".shell");
    await waitFor(() => {
      expect(shell?.style.getPropertyValue("--plugin-side-width")).toBe("27rem");
    });
  });
});

describe("side panel", () => {
  it("shows the members until a plugin opens its panel", async () => {
    const { container } = mount();
    expect(screen.getByText("Members")).toBeVisible();
    show("sketch");
    expect(await screen.findByRole("region", { name: "Sketchboard side panel" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Sketchboard" })).toBeVisible();
    expect(screen.getByText("Unofficial")).toBeVisible();
    expect(screen.getByText("Members")).not.toBeVisible();
    expect(container.querySelector("iframe")).not.toBeNull();
    expect(container.querySelector<HTMLElement>(".shell")?.dataset.pluginSidePanel).toBe("true");
  });

  it("has one panel at a time: the newest wins", async () => {
    mount();
    show("sketch");
    show("dice");
    expect(await screen.findByRole("region", { name: "Dice side panel" })).toBeVisible();
    expect(screen.queryByRole("region", { name: "Sketchboard side panel" })).toBeNull();
  });

  it("closes with the button, tells the plugin and shows the members again", async () => {
    mount();
    show("sketch");
    fireEvent.click(await screen.findByRole("button", { name: "Close Sketchboard side panel" }));
    expect(api.sendEvent).toHaveBeenCalledWith("sketch", {
      type: "surfaceClosed",
      place: "sidePanel",
    });
    expect(screen.queryByRole("region", { name: "Sketchboard side panel" })).toBeNull();
    expect(screen.getByText("Members")).toBeVisible();
    // A new page from the plugin opens it again.
    show("sketch");
    expect(await screen.findByRole("region", { name: "Sketchboard side panel" })).toBeVisible();
  });

  it("resizes by keyboard in 1 rem steps and remembers the width", async () => {
    const { container } = mount();
    show("sketch");
    const handle = await screen.findByRole("separator", { name: "Resize Sketchboard side panel" });
    const shell = container.querySelector<HTMLElement>(".shell");
    expect(shell?.style.getPropertyValue("--plugin-side-width")).toBe("24rem");
    fireEvent.keyDown(handle, { key: "ArrowLeft" });
    expect(shell?.style.getPropertyValue("--plugin-side-width")).toBe("25rem");
    fireEvent.keyDown(handle, { key: "ArrowRight" });
    fireEvent.keyDown(handle, { key: "ArrowRight" });
    expect(shell?.style.getPropertyValue("--plugin-side-width")).toBe("23rem");
    await waitFor(() => {
      expect(api.widths.get("sketch")).toBe(23);
    });
    fireEvent.keyDown(handle, { key: "End" });
    expect(handle).toHaveAttribute("aria-valuenow", "32");
  });

  it("has an options menu with the plugin details and a close entry", async () => {
    mount();
    show("sketch");
    fireEvent.click(await screen.findByRole("button", { name: "Sketchboard options" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Plugin details" }));
    expect(requestPluginScreen).toHaveBeenCalledWith({ kind: "installed", plugin: "sketch" });
    fireEvent.click(screen.getByRole("button", { name: "Sketchboard options" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Close panel" }));
    expect(api.sendEvent).toHaveBeenCalledWith("sketch", {
      type: "surfaceClosed",
      place: "sidePanel",
    });
    expect(screen.queryByRole("region", { name: "Sketchboard side panel" })).toBeNull();
  });

  it("shows no status line while the plugin sent none", async () => {
    const { container } = mount();
    show("sketch");
    await screen.findByRole("region", { name: "Sketchboard side panel" });
    expect(container.querySelector(".plugin-sidepanel__foot")).toBeNull();
  });

  it("returns focus to the opener on close", async () => {
    render(
      <div className="shell">
        <button type="button">Open</button>
        <aside className="shell__members">
          <SidePanelHost>
            <span>Members</span>
          </SidePanelHost>
        </aside>
      </div>,
    );
    const opener = screen.getByRole("button", { name: "Open" });
    opener.focus();
    show("sketch");
    fireEvent.click(await screen.findByRole("button", { name: "Close Sketchboard side panel" }));
    expect(opener).toHaveFocus();
  });

  it("has no accessibility violations", async () => {
    const { container } = mount();
    show("sketch");
    await screen.findByRole("region", { name: "Sketchboard side panel" });
    const results = await axe.run(container, { iframes: false });
    expect(results.violations.map((v) => v.id)).toEqual([]);
  });
});
