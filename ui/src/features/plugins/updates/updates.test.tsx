import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type InstalledPlugin, type PluginUpdate } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { resetPluginStore } from "../pluginStore";
import { UpdatesTab } from "./UpdatesTab";

const plugin: InstalledPlugin = {
  id: "poll",
  name: "Poll",
  version: "1.0.0",
  source: { kind: "official" },
  status: { kind: "running" },
  enabled: true,
  disabledReason: null,
  required: [],
  optional: [],
  granted: ["ui.panel"],
  visibility: "everyone",
  pinned: false,
  notifications: true,
  installedAt: 0,
  badge: false,
};

const update: PluginUpdate = {
  id: "poll",
  name: "Poll",
  from: "1.0.0",
  to: "1.2.0",
  source: { kind: "official" },
  size: 340_000,
  approval: { kind: "newPermissions", permissions: ["http.allow"] },
  failed: null,
};

function setup(updates: PluginUpdate[]) {
  const api = createMockApi();
  vi.spyOn(api, "pluginsList").mockResolvedValue({
    noPlugins: false,
    allOff: false,
    plugins: [plugin],
  });
  vi.spyOn(api, "pluginUpdates").mockResolvedValue(updates);
  const apply = vi.spyOn(api, "pluginApplyUpdate").mockResolvedValue(null);
  const pin = vi.spyOn(api, "pluginSetPinned").mockResolvedValue(null);
  setApi(api);
  return { apply, pin };
}

beforeEach(async () => {
  await applyLanguage("en");
  resetPluginStore();
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
});

describe("UpdatesTab", () => {
  it("says everything is up to date", async () => {
    setup([]);
    render(<UpdatesTab mode="auto" onModeChange={() => undefined} />);
    expect(await screen.findByText("All plugins are up to date.")).toBeInTheDocument();
  });

  it("asks before installing an update with new permissions", async () => {
    const { apply } = setup([update]);
    const { container } = render(<UpdatesTab mode="auto" onModeChange={() => undefined} />);
    fireEvent.click(await screen.findByRole("button", { name: "Review and update" }));
    expect(apply).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Approve and update" }));
    await waitFor(() => {
      expect(apply).toHaveBeenCalledWith("poll", ["ui.panel"]);
    });
    expect(await axeViolations(container)).toEqual([]);
  });

  it("installs an update without new permissions in one click", async () => {
    const { apply } = setup([{ ...update, approval: null }]);
    render(<UpdatesTab mode="notify" onModeChange={() => undefined} />);
    fireEvent.click(await screen.findByRole("button", { name: "Update" }));
    await waitFor(() => {
      expect(apply).toHaveBeenCalledWith("poll", ["ui.panel"]);
    });
  });

  it("shows the reason of a rolled-back update", async () => {
    setup([{ ...update, approval: null, failed: "it crashed" }]);
    render(<UpdatesTab mode="auto" onModeChange={() => undefined} />);
    expect(await screen.findByText(/previous version was restored/)).toHaveTextContent(
      "it crashed",
    );
  });

  it("pins a version and reports the update setting", async () => {
    const { pin } = setup([]);
    const onModeChange = vi.fn();
    render(<UpdatesTab mode="auto" onModeChange={onModeChange} />);
    fireEvent.click(await screen.findByRole("switch", { name: /Keep Poll on this version/ }));
    expect(pin).toHaveBeenCalledWith("poll", true);
    fireEvent.click(screen.getByRole("radio", { name: /Notify only/ }));
    expect(onModeChange).toHaveBeenCalledWith("notify");
  });
});
