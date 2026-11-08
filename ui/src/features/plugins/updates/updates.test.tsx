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
  appMenuItems: 0,
  webSize: null,
  themedPages: false,
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
  needsMumcord: null,
  hold: null,
  review: {
    origins: [],
    otherDomain: false,
    permissionEffects: [],
    recentChange: null,
    risk: null,
    needsHighRiskConfirm: false,
    highRiskConfirmed: false,
    notesUrl: null,
  },
};

function setup(updates: PluginUpdate[], installed: InstalledPlugin = plugin) {
  const api = createMockApi();
  vi.spyOn(api, "pluginsList").mockResolvedValue({
    noPlugins: false,
    allOff: false,
    plugins: [installed],
    refused: [],
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
    expect(await screen.findByText("Asks for more permissions")).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "New permissions" })).toHaveTextContent("+");
    expect(apply).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: /^Review and update\s+Poll/u }));
    fireEvent.click(await screen.findByRole("button", { name: "Allow and update" }));
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
      refused: [],
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
    fireEvent.click(await screen.findByRole("button", { name: /^Update\s+Poll/u }));
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

const risk = { fromValue: 37, fromBand: "medium", toValue: 71, toBand: "high" } as const;

const held: PluginUpdate = {
  ...update,
  id: "poll",
  approval: { kind: "newPermissions", permissions: ["ui.webview", "peers.relay"] },
  source: { kind: "repo", repositoryId: 1, name: "Lantern Plugin Garden" },
  hold: { kind: "riskUp", from: "medium", to: "high" },
  review: {
    origins: [],
    otherDomain: false,
    permissionEffects: [],
    recentChange: null,
    risk,
    needsHighRiskConfirm: true,
    highRiskConfirmed: false,
    notesUrl: null,
  },
};

const websites: PluginUpdate = {
  ...update,
  approval: null,
  hold: { kind: "newWebsites", count: 2 },
  review: {
    origins: [
      { origin: "https://api.poll.example", new: false, purpose: null, otherDomain: false },
      { origin: "https://stats.poll.example", new: true, purpose: null, otherDomain: false },
      { origin: "wss://voice-lab.example", new: true, purpose: null, otherDomain: true },
    ],
    otherDomain: true,
    permissionEffects: [],
    recentChange: null,
    risk: null,
    needsHighRiskConfirm: false,
    highRiskConfirmed: false,
    notesUrl: null,
  },
};

function renderTab() {
  return render(<UpdatesTab mode="auto" onModeChange={() => undefined} />);
}

describe("UpdatesTab: held updates and the review (FR-9.18, FR-9.82, FR-9.83)", () => {
  it("shows an update that needs a newer Mumcord without an install button", async () => {
    setup([{ ...update, approval: null, needsMumcord: "0.8.0" }]);
    const { container } = renderTab();
    expect(await screen.findByText("Update available after updating Mumcord")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /^(Update|Review and update)\s+Poll/u }),
    ).toBeNull();
    expect(await axeViolations(container)).toEqual([]);
  });

  it.each([
    [
      { kind: "newWebsites", count: 2 },
      "This update wants to talk to 2 new websites. Poll stays on 1.0.0 until you decide.",
    ],
    [
      { kind: "riskUp", from: "medium", to: "high" },
      "Not installed automatically: the risk went up from Medium to High. Review it first.",
    ],
    [
      { kind: "scoreUp", from: 12, to: 25 },
      "Not installed automatically: the score went up from 12 to 25. Review it first.",
    ],
    [
      { kind: "scoreUnknown" },
      "Not installed automatically: the security score of this version could not be worked out. Review it first.",
    ],
  ] as const)("says why a held update waits: %o", async (hold, text) => {
    setup([{ ...update, approval: null, hold }]);
    const { container } = renderTab();
    expect(await screen.findByText(text)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Review and update\s+Poll/u })).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("offers no install for an update the safety setting blocks", async () => {
    setup([{ ...update, approval: null, hold: { kind: "blockedBySafety" } }]);
    const { container } = renderTab();
    expect(await screen.findByText(/Don't allow high-risk plugins/)).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /^(Update|Review and update)\s+Poll/u }),
    ).toBeNull();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("shows the score before and after with the rise in points", async () => {
    setup([held]);
    const { container } = renderTab();
    expect(await screen.findByText("Security score:")).toBeInTheDocument();
    expect(
      screen.getByText("Security score 37 out of 100. Medium risk. Higher means more risk."),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Security score 71 out of 100. High risk. Higher means more risk."),
    ).toBeInTheDocument();
    expect(screen.getByText("plus 34 points, adds risk")).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("says unchanged when the score stays the same", async () => {
    setup([
      {
        ...update,
        approval: null,
        review: {
          ...update.review,
          risk: { fromValue: 2, fromBand: "low", toValue: 2, toBand: "low" },
        },
      },
    ]);
    renderTab();
    expect(await screen.findByText("unchanged")).toBeInTheDocument();
  });

  it("marks new websites and a website off the plugin's own domain", async () => {
    const { apply } = setup([websites]);
    renderTab();
    fireEvent.click(await screen.findByRole("button", { name: /^Review and update\s+Poll/u }));
    const dialog = await screen.findByRole("dialog", { name: "Update Poll to 1.2.0?" });
    expect(dialog).toHaveTextContent("This update wants to talk to 2 new websites.");
    expect(dialog).toHaveTextContent("One of them is not on the plugin's own domain.");
    const list = screen.getByRole("list", { name: "Websites" });
    expect(list.querySelectorAll("li")).toHaveLength(3);
    expect(screen.getAllByText("New")).toHaveLength(2);
    expect(dialog).toHaveTextContent("No other permissions change.");
    expect(dialog).toHaveTextContent("Skipping keeps 1.0.0 working.");
    expect(await axeViolations(dialog)).toEqual([]);
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Allow and update" })).toBeEnabled();
    });
    fireEvent.click(screen.getByRole("button", { name: "Allow and update" }));
    await waitFor(() => {
      expect(apply).toHaveBeenCalledWith("poll", ["ui.panel"]);
    });
  });

  it("reads the new websites' purposes from the prepared review and quotes the off-domain one", async () => {
    setup([websites]);
    const prepare = vi.spyOn(getApi(), "pluginUpdateReviewPrepare").mockResolvedValue({
      ...websites.review,
      origins: [
        {
          origin: "https://api.poll.example",
          new: false,
          purpose: "match history",
          otherDomain: false,
        },
        {
          origin: "https://stats.poll.example",
          new: true,
          purpose: "rankings",
          otherDomain: false,
        },
        {
          origin: "wss://voice-lab.example",
          new: true,
          purpose: "experimental features",
          otherDomain: true,
        },
      ],
    });
    renderTab();
    fireEvent.click(await screen.findByRole("button", { name: /^Review and update\s+Poll/u }));
    const dialog = await screen.findByRole("dialog");
    expect(await screen.findByText("rankings")).toBeInTheDocument();
    expect(prepare).toHaveBeenCalledWith("poll", "1.2.0");
    expect(screen.getByText("“experimental features”")).toBeInTheDocument();
    expect(screen.getByText("match history")).toBeInTheDocument();
    expect(dialog).not.toHaveTextContent("“rankings”");
  });

  it("links to the release notes through the safe external-link path, only when there are some", async () => {
    setup([
      { ...websites, review: { ...websites.review, notesUrl: "https://poll.example/notes/1.2.0" } },
    ]);
    const openLink = vi.spyOn(getApi(), "openLink").mockResolvedValue(null);
    renderTab();
    fireEvent.click(await screen.findByRole("button", { name: /^Review and update\s+Poll/u }));
    const link = await screen.findByRole("link", { name: "What's new in 1.2.0" });
    expect(link.parentElement).toHaveTextContent(
      /^No other permissions change\. What's new in 1\.2\.0$/u,
    );
    fireEvent.click(link);
    expect(openLink).toHaveBeenCalledWith("https://poll.example/notes/1.2.0");
  });

  it("shows no release-notes link when the author gave none", async () => {
    setup([held]);
    renderTab();
    fireEvent.click(await screen.findByRole("button", { name: /^Review and update\s+Poll/u }));
    await screen.findByRole("dialog");
    expect(screen.queryByRole("link", { name: /What's new/u })).not.toBeInTheDocument();
  });

  it("shows an unofficial source as a dashed chip", async () => {
    setup([held]);
    renderTab();
    fireEvent.click(await screen.findByRole("button", { name: /^Review and update\s+Poll/u }));
    const chip = await screen.findByText("Unofficial · Lantern Plugin Garden");
    expect(chip).toHaveClass("plugin-prompt-chip--unofficial");
  });

  it("keeps the old version when the user skips", async () => {
    const { apply } = setup([websites]);
    const skip = vi.spyOn(getApi(), "pluginSkipUpdateVersion").mockResolvedValue(null);
    renderTab();
    fireEvent.click(await screen.findByRole("button", { name: /^Review and update\s+Poll/u }));
    fireEvent.click(await screen.findByRole("button", { name: "Skip this version" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(skip).toHaveBeenCalledWith("poll", "1.2.0");
    expect(apply).not.toHaveBeenCalled();
  });

  it("renders origin purposes, permission effects and the recent change once sent", async () => {
    setup([
      {
        ...held,
        review: {
          ...held.review,
          origins: [
            {
              origin: "https://api.poll.example",
              new: false,
              purpose: "match history",
              otherDomain: false,
            },
          ],
          permissionEffects: [
            { permission: "ui.webview", points: 4 },
            { permission: "peers.relay", points: 14 },
          ],
          recentChange: { points: 18, trustPercent: 90, permissions: ["ui.webview"] },
        },
      },
    ]);
    renderTab();
    fireEvent.click(await screen.findByRole("button", { name: /^Review and update\s+Poll/u }));
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent("match history");
    expect(dialog).toHaveTextContent("plus 4 points, adds risk");
    expect(dialog).toHaveTextContent(
      "It also counts as a recent change: new permissions in the last 5 versions add another +18 points. Together with the +18 above (× 0.90) that is the +34.",
    );
  });

  it("needs the tick before an update to high risk, then confirms before installing", async () => {
    const { apply } = setup([held]);
    const confirm = vi.spyOn(getApi(), "pluginConfirmUpdateHighRisk").mockResolvedValue(null);
    renderTab();
    expect(
      await screen.findByText(
        "Not installed automatically: the risk went up from Medium to High. Review it first.",
      ),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /^Review and update\s+Poll/u }));
    const dialog = await screen.findByRole("dialog", { name: "Update Poll to 1.2.0?" });
    expect(dialog).toHaveTextContent("Unofficial · Lantern Plugin Garden");
    expect(dialog).toHaveTextContent("The risk goes up by 34 points");
    expect(screen.getByRole("meter", { name: "Security score now" })).toHaveAttribute(
      "aria-valuenow",
      "37",
    );
    expect(screen.getByRole("meter", { name: "Security score after the update" })).toHaveAttribute(
      "aria-valuenow",
      "71",
    );
    expect(await axeViolations(dialog)).toEqual([]);
    const updateAnyway = screen.getByRole("button", { name: "Update anyway" });
    expect(updateAnyway).toBeDisabled();
    fireEvent.click(
      screen.getByRole("checkbox", {
        name: "I understand the risk went up and I still trust the author",
      }),
    );
    expect(updateAnyway).toBeEnabled();
    fireEvent.click(updateAnyway);
    await waitFor(() => {
      expect(apply).toHaveBeenCalledWith("poll", ["ui.panel"]);
    });
    expect(confirm).toHaveBeenCalledWith("poll", "1.2.0");
    expect(confirm.mock.invocationCallOrder[0]).toBeLessThan(
      apply.mock.invocationCallOrder[0] ?? 0,
    );
  });

  it("closes the high-risk review with Not now and Escape", async () => {
    const { apply } = setup([held]);
    renderTab();
    fireEvent.click(await screen.findByRole("button", { name: /^Review and update\s+Poll/u }));
    fireEvent.click(await screen.findByRole("button", { name: "Not now" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /^Review and update\s+Poll/u }));
    fireEvent.keyDown(await screen.findByRole("dialog"), { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(apply).not.toHaveBeenCalled();
  });

  it("reports a failed install in the review and keeps it open", async () => {
    const { apply } = setup([websites]);
    apply.mockRejectedValue(new Error("boom"));
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    renderTab();
    fireEvent.click(await screen.findByRole("button", { name: /^Review and update\s+Poll/u }));
    fireEvent.click(await screen.findByRole("button", { name: "Allow and update" }));
    expect(
      await screen.findByText("Poll could not be updated. Please try again."),
    ).toBeInTheDocument();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });
});
