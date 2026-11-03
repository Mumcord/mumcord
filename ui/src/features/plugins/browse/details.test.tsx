import { render, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../../api";
import type { PluginScore, PluginScored } from "../../../bindings/commands";
import { PreferencesContext, type Preferences } from "../../../app/preferences";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { indexed } from "./fixtures";
import { PluginDetails } from "./PluginDetails";
import { resetIndexImages } from "./useIndexImage";

let api: MockApi;
const preferences = { language: "en" } as Preferences;
function Wrapper({ children }: { children: ReactNode }) {
  return <PreferencesContext value={preferences}>{children}</PreferencesContext>;
}

const score: PluginScore = {
  value: 72,
  band: "high",
  e: 63,
  f: 1.15,
  h: 0,
  factors: [{ kind: "websites", effect: { kind: "points", value: 6 }, detailKey: "origins" }],
  version: 1,
};
const scored: PluginScored = { kind: "scored", score };

const unofficial = indexed("overlay", {
  name: "Overlay Pals",
  source: { kind: "repo", repositoryId: 1, name: "Lantern Plugin Garden" },
  reviewed: false,
  size: 12 * 1024 * 1024,
  permissions: ["ui.stage", "ui.dialog", "ui.app-menu", "ui.webview", "net.origins", "peers.relay"],
  origins: ["https://api.example.com", "wss://play.example.com"],
});

beforeEach(async () => {
  await applyLanguage("en");
  resetIndexImages();
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
});

function open(plugin = unofficial) {
  return render(
    <PluginDetails plugin={plugin} installed={false} onBack={vi.fn()} onInstall={vi.fn()} />,
    {
      wrapper: Wrapper,
    },
  );
}

describe("Browse plugin details", () => {
  it("fills the placeholders of the permission explanations", () => {
    open({ ...unofficial, permissions: ["ui.stage", "ui.app-menu"] });
    expect(document.body.textContent).not.toContain("{{");
    expect(document.body.textContent).toContain(`who have ${unofficial.name} can join`);
    expect(document.body.textContent).toContain("Up to 2 items");
  });

  it("lists websites, places, own code and relay, and shows the score card", async () => {
    const spy = vi.spyOn(api, "pluginScore").mockResolvedValue(scored);
    const { container } = open();

    const websites = screen.getByRole("region", { name: "Websites it talks to" });
    expect(within(websites).getByText("https://api.example.com")).toBeTruthy();
    expect(within(websites).getByText("wss://play.example.com")).toBeTruthy();
    const places = screen.getByRole("region", { name: "Where it shows" });
    expect(
      within(places)
        .getAllByRole("listitem")
        .map((li) => li.textContent),
    ).toEqual(["Media area", "Dialog", "Apps menu"]);
    const own = screen.getByRole("region", { name: "Own code" });
    expect(within(own).getByText(/web\/index\.html · no access to Mumcord/)).toBeTruthy();
    expect(within(own).getByText("Asks to use it · you decide when you install")).toBeTruthy();

    const card = await screen.findByRole("region", { name: "Security score" });
    expect(spy).toHaveBeenCalledWith("overlay", "1.0.0", "listing");
    expect(within(card).getByRole("meter")).toBeTruthy();
    expect(within(card).getAllByRole("table").length).toBeGreaterThan(0);
    expect(within(card).getByText(/2 websites/)).toBeTruthy();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("shows the review instead of a number for an official plugin", async () => {
    const spy = vi.spyOn(api, "pluginScore").mockResolvedValue({ kind: "official" });
    open(indexed("rollcall", { name: "Roll Call", permissions: ["ui.chat-card"] }));
    const card = await screen.findByRole("region", { name: "Security" });
    expect(within(card).getByText(/do not get a score/)).toBeTruthy();
    expect(screen.queryByRole("meter")).toBeNull();
    expect(screen.queryByRole("region", { name: "Own code" })).toBeNull();
    expect(spy).not.toHaveBeenCalled();
  });

  it("leaves the card out when the score cannot be loaded", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => undefined);
    open();
    await waitFor(() => {
      expect(error).toHaveBeenCalled();
    });
    expect(screen.queryByRole("region", { name: "Security score" })).toBeNull();
  });
});
