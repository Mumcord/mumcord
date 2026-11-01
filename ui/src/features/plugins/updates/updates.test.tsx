import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createMockApi,
  getApi,
  setApi,
  type InstalledPlugin,
  type PluginUpdate,
} from "../../../api";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { resetPluginStore } from "../pluginStore";
import { UpdatesBadge } from "./UpdatesBadge";
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
  revocation: null,
  icon: null,
  description: "",
  authors: [],
  chatPatterns: [],
  offered: [],
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

function setup(updates: PluginUpdate[], installed: InstalledPlugin = plugin) {
  const api = createMockApi();
  vi.spyOn(api, "pluginsList").mockResolvedValue({
    noPlugins: false,
    allOff: false,
    plugins: [installed],
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

  it("shows the new permission before installing an update that needs it", async () => {
    const { apply } = setup([update]);
    const { container } = render(<UpdatesTab mode="auto" onModeChange={() => undefined} />);
    expect(await screen.findByText("Needs a new permission")).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "New permissions" })).toHaveTextContent("+");
    expect(apply).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Review and update" }));
    await waitFor(() => {
      expect(apply).toHaveBeenCalledWith("poll", ["ui.panel"]);
    });
    expect(await axeViolations(container)).toEqual([]);
  });

  it("says when an update needs no new permission", async () => {
    setup([{ ...update, approval: null }]);
    render(<UpdatesTab mode="auto" onModeChange={() => undefined} />);
    expect(await screen.findByText("No new permissions")).toBeInTheDocument();
  });

  it("shows a banner for a revoked plugin and removes it keeping its data", async () => {
    const api = createMockApi();
    vi.spyOn(api, "pluginsList").mockResolvedValue({
      noPlugins: false,
      allOff: false,
      plugins: [{ ...plugin, enabled: false, disabledReason: "revoked", revocation: "Leaks" }],
    });
    vi.spyOn(api, "pluginUpdates").mockResolvedValue([]);
    const remove = vi.spyOn(api, "pluginRemove").mockResolvedValue(null);
    setApi(api);
    render(<UpdatesTab mode="auto" onModeChange={() => undefined} />);
    expect(await screen.findByText("Poll was turned off")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Remove/ }));
    expect(remove).toHaveBeenCalledWith("poll", false);
  });

  it("checks for updates on demand", async () => {
    const api = createMockApi();
    vi.spyOn(api, "pluginUpdates").mockResolvedValue([]);
    const check = vi.spyOn(api, "pluginCheckNow").mockResolvedValue(null);
    setApi(api);
    render(<UpdatesTab mode="notify" onModeChange={() => undefined} />);
    fireEvent.click(await screen.findByRole("button", { name: "Check now" }));
    await waitFor(() => {
      expect(check).toHaveBeenCalled();
    });
    expect(await screen.findByText(/Last checked/)).toBeInTheDocument();
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

  describe("new optional permissions from an update", () => {
    const offered: InstalledPlugin = {
      ...plugin,
      version: "1.2.0",
      optional: [
        { id: "chat.triggers", suggested: true },
        { id: "notify", suggested: false },
      ],
      offered: ["chat.triggers", "notify"],
    };
    const triggers = /New optional permission: Read chat messages that start with its trigger/;
    const notify = /New optional permission: Show notifications/;

    function setupOffer() {
      const { apply } = setup([], offered);
      const set = vi.spyOn(getApi(), "pluginSetPermissions").mockResolvedValue(null);
      return { apply, set };
    }

    it("offers them with the suggested ones switched on and grants nothing by itself", async () => {
      const { set } = setupOffer();
      const { container } = render(<UpdatesTab mode="auto" onModeChange={() => undefined} />);
      expect(await screen.findByRole("switch", { name: triggers })).toBeChecked();
      expect(screen.getByRole("switch", { name: notify })).not.toBeChecked();
      expect(screen.getByText(/Poll 1.2.0 can do more now/)).toBeInTheDocument();
      expect(set).not.toHaveBeenCalled();
      expect(await axeViolations(container)).toEqual([]);
    });

    it("turns on what the user chose on Save", async () => {
      const { set } = setupOffer();
      render(<UpdatesTab mode="auto" onModeChange={() => undefined} />);
      fireEvent.click(await screen.findByRole("switch", { name: notify }));
      fireEvent.click(screen.getByRole("switch", { name: triggers }));
      fireEvent.click(screen.getByRole("button", { name: /Save/ }));
      await waitFor(() => {
        expect(set).toHaveBeenCalledWith("poll", ["ui.panel", "notify"]);
      });
    });

    it("keeps them off on Not now", async () => {
      const { set } = setupOffer();
      render(<UpdatesTab mode="auto" onModeChange={() => undefined} />);
      fireEvent.click(await screen.findByRole("button", { name: /Not now/ }));
      await waitFor(() => {
        expect(set).toHaveBeenCalledWith("poll", ["ui.panel"]);
      });
    });

    it("says so when the permissions could not be changed", async () => {
      const { set } = setupOffer();
      set.mockRejectedValue(new Error("boom"));
      vi.spyOn(console, "error").mockImplementation(() => undefined);
      render(<UpdatesTab mode="auto" onModeChange={() => undefined} />);
      fireEvent.click(await screen.findByRole("button", { name: /Save/ }));
      expect(await screen.findByRole("alert")).toHaveTextContent(
        "The permissions of Poll could not be changed.",
      );
    });
  });
});

describe("UpdatesBadge", () => {
  it("counts waiting updates and new optional permissions", async () => {
    setup([update], { ...plugin, id: "dice", offered: ["chat.send"] });
    render(<UpdatesBadge />);
    expect(await screen.findByText("2")).toBeTruthy();
    expect(screen.getByText("Updates")).toBeTruthy();
  });

  it("shows nothing when there is nothing to update", async () => {
    setup([]);
    const { container } = render(<UpdatesBadge />);
    await waitFor(() => {
      expect(getApi().pluginUpdates).toHaveBeenCalled();
    });
    expect(container.textContent).toBe("");
  });
});
