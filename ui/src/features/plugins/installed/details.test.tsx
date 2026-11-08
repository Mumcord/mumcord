import { render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type InstalledPlugin, type MockApi } from "../../../api";
import type { PluginScored } from "../../../bindings/commands";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { resetPluginStore } from "../pluginStore";
import { PluginDetails } from "./PluginDetails";

let api: MockApi;

const brawl: InstalledPlugin = {
  id: "brawl",
  name: "Board Brawl",
  version: "2.1.0",
  source: { kind: "repo", repositoryId: 1, name: "Meeple Works" },
  status: { kind: "running" },
  enabled: true,
  disabledReason: null,
  required: ["ui.stage", "ui.webview", "net.origins"],
  optional: [{ id: "peers.relay", suggested: false }],
  granted: ["ui.stage", "ui.webview", "net.origins"],
  visibility: "everyone",
  pinned: false,
  notifications: true,
  installedAt: Date.UTC(2026, 10, 2, 12),
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

const scored: PluginScored = {
  kind: "scored",
  score: {
    value: 40,
    band: "medium",
    e: 40,
    f: 1,
    h: 0,
    factors: [{ kind: "websites", effect: { kind: "points", value: 6 }, detailKey: "origins" }],
    version: 1,
  },
};

function today(): string {
  const d = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${String(d.getFullYear())}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

beforeEach(async () => {
  await applyLanguage("en");
  resetPluginStore();
  api = createMockApi();
  setApi(api);
  vi.spyOn(api, "pluginScore").mockResolvedValue(scored);
  vi.spyOn(api, "pluginCommands").mockResolvedValue([]);
  vi.spyOn(api, "pluginWebsites").mockResolvedValue({
    origins: [{ origin: "https://api.boardbrawl.example", purpose: "accounts and match history" }],
    days: [
      {
        day: today(),
        requests: 148,
        blocked: 2,
        lastRequestAt: Date.now(),
        lastHost: "api.boardbrawl.example",
      },
    ],
  });
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
});

describe("Installed plugin details", () => {
  it("shows websites with today's counters, places, own code and the score card", async () => {
    const { container } = render(
      <PluginDetails plugin={brawl} run={vi.fn().mockResolvedValue(true)} busy={false} />,
    );
    const websites = await screen.findByRole("region", { name: "Websites it talks to" });
    expect(within(websites).getByText("https://api.boardbrawl.example")).toBeTruthy();
    expect(within(websites).getByText("accounts and match history")).toBeTruthy();
    expect(within(websites).getByText("148 (blocked: 2)")).toBeTruthy();
    expect(within(websites).getByText(/· api\.boardbrawl\.example$/)).toBeTruthy();
    expect(
      within(screen.getByRole("region", { name: "Where it shows" })).getByText("Media area"),
    ).toBeTruthy();
    const own = screen.getByRole("region", { name: "Own code" });
    expect(within(own).getByText(/web\/index\.html/)).toBeTruthy();
    expect(within(own).getByText(/Off/)).toBeTruthy();
    expect(await screen.findByRole("region", { name: "Security score" })).toBeTruthy();
    await waitFor(async () => {
      expect(await axeViolations(container)).toEqual([]);
    });
  });

  it("shows the Apps-menu item count and the size of the web pages", async () => {
    render(
      <PluginDetails
        plugin={{
          ...brawl,
          required: [...brawl.required, "ui.app-menu"],
          appMenuItems: 2,
          webSize: 1_800_000,
        }}
        run={vi.fn().mockResolvedValue(true)}
        busy={false}
      />,
    );
    const places = await screen.findByRole("region", { name: "Where it shows" });
    expect(within(places).getByText("Apps menu · 2 items")).toBeTruthy();
    const own = screen.getByRole("region", { name: "Own code" });
    expect(within(own).getByText(/web\/index\.html · 1\.8 MB/)).toBeTruthy();
  });

  it("moves focus to the permissions from 'Turn on in permissions'", async () => {
    render(<PluginDetails plugin={brawl} run={vi.fn().mockResolvedValue(true)} busy={false} />);
    (await screen.findByRole("button", { name: "Turn on in permissions" })).click();
    expect(document.activeElement?.textContent).toBe("Permissions");
  });

  it("shows no score card for an official plugin", async () => {
    vi.spyOn(api, "pluginWebsites").mockResolvedValue({ origins: [], days: [] });
    render(
      <PluginDetails
        plugin={{
          ...brawl,
          source: { kind: "official" },
          required: ["ui.panel"],
          optional: [],
          granted: [],
        }}
        run={vi.fn().mockResolvedValue(true)}
        busy={false}
      />,
    );
    await screen.findByRole("region", { name: "Where it shows" }).catch(() => null);
    expect(screen.queryByRole("region", { name: "Security score" })).toBeNull();
    expect(screen.queryByRole("region", { name: "Websites it talks to" })).toBeNull();
  });
});
