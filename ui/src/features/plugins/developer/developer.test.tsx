import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type InstalledPlugin, type MockApi } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { resetPluginStore } from "../pluginStore";
import { DeveloperTab } from "./DeveloperTab";

let api: MockApi;

const devPlugin: InstalledPlugin = {
  id: "my-dev",
  name: "My Dev",
  version: "0.1.0",
  source: { kind: "dev", folder: "/home/me/my-dev" },
  status: { kind: "running" },
  enabled: true,
  disabledReason: null,
  required: [],
  optional: [],
  granted: [],
  visibility: "private",
  pinned: false,
  notifications: false,
  installedAt: null,
  badge: false,
} as unknown as InstalledPlugin;

beforeEach(async () => {
  await applyLanguage("en");
  resetPluginStore();
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
});

function withDevPlugin(): void {
  vi.spyOn(api, "pluginsList").mockResolvedValue({
    noPlugins: false,
    allOff: false,
    plugins: [devPlugin],
  });
  vi.spyOn(api, "pluginDiagnostics").mockResolvedValue({
    log: [{ level: "info", text: "started" }],
  } as never);
}

describe("DeveloperTab", () => {
  it("shows the permanent Developer plugin label, the log and no violations", async () => {
    withDevPlugin();
    const { container } = render(<DeveloperTab />);
    expect(await screen.findByText("Developer plugin")).toBeInTheDocument();
    expect(await screen.findByText("started")).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("reloads and unloads the selected plugin", async () => {
    withDevPlugin();
    const reload = vi.spyOn(api, "pluginDevReload").mockResolvedValue(null);
    const unload = vi.spyOn(api, "pluginDevUnload").mockResolvedValue(null);
    render(<DeveloperTab />);
    fireEvent.click(await screen.findByRole("button", { name: "Reload My Dev" }));
    await waitFor(() => {
      expect(reload).toHaveBeenCalledWith("my-dev");
    });
    const unloadButton = screen.getByRole("button", { name: "Unload My Dev" });
    await waitFor(() => {
      expect(unloadButton).toBeEnabled();
    });
    fireEvent.click(unloadButton);
    await waitFor(() => {
      expect(unload).toHaveBeenCalledWith("my-dev");
    });
  });

  it("loads the folder the user types in browser dev", async () => {
    vi.spyOn(window, "prompt").mockReturnValue("/home/me/new-plugin");
    const load = vi.spyOn(api, "pluginDevLoad").mockResolvedValue("new-plugin");
    render(<DeveloperTab />);
    fireEvent.click(await screen.findByRole("button", { name: "Load plugin folder…" }));
    await waitFor(() => {
      expect(load).toHaveBeenCalledWith("/home/me/new-plugin");
    });
  });

  it("shows a readable error when loading fails", async () => {
    vi.spyOn(window, "prompt").mockReturnValue("/x");
    vi.spyOn(api, "pluginDevLoad").mockRejectedValue(new Error("boom"));
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    render(<DeveloperTab />);
    fireEvent.click(await screen.findByRole("button", { name: "Load plugin folder…" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("could not be loaded");
  });
});
