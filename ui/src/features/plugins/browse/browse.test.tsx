import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, EngineError, setApi, type MockApi } from "../../../api";
import { PreferencesContext, type Preferences } from "../../../app/preferences";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { setSettingsOpener } from "../../settings/settingsOpener";
import {
  resetPluginRequestStore,
  routePluginRequest,
  usePendingPluginRequest,
} from "../pluginRequestStore";
import { resetPluginStore } from "../pluginStore";
import { BrowseTab } from "./BrowseTab";
import { usePluginDeepLinks } from "./deepLink";
import { visiblePlugins } from "./filter";
import { index, indexed } from "./fixtures";
import { resetPluginIndex } from "./indexStore";
import { resetIndexImages } from "./useIndexImage";

let api: MockApi;

const dice = indexed("dice", { categories: ["games"], summary: "Roll dice in the chat" });
const poll = indexed("poll", { categories: ["chat"], summary: "Quick polls" });
const echolings = indexed("echolings", { categories: ["fun"], summary: "Virtual pets" });

const preferences = { language: "en" } as Preferences;
function Wrapper({ children }: { children: ReactNode }) {
  return <PreferencesContext value={preferences}>{children}</PreferencesContext>;
}

beforeEach(async () => {
  await applyLanguage("en");
  resetPluginStore();
  resetPluginIndex();
  resetIndexImages();
  resetPluginRequestStore();
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  vi.restoreAllMocks();
  setSettingsOpener(null);
  setApi(undefined);
});

function withIndex(plugins = [dice, poll, echolings], extra = {}) {
  return vi.spyOn(api, "pluginsIndex").mockResolvedValue(index(plugins, extra));
}

describe("visiblePlugins", () => {
  it("puts Echolings first and filters by category and search", () => {
    const all = [dice, poll, echolings];
    expect(visiblePlugins(all, "all", "").map((p) => p.id)).toEqual(["echolings", "dice", "poll"]);
    expect(visiblePlugins(all, "chat", "").map((p) => p.id)).toEqual(["poll"]);
    expect(visiblePlugins(all, "all", "ROLL").map((p) => p.id)).toEqual(["dice"]);
    // A third-party plugin with the same id is never highlighted.
    const copy = indexed("echolings", { source: { kind: "repo", repositoryId: 1, name: "Fans" } });
    expect(visiblePlugins([dice, copy], "all", "").map((p) => p.id)).toEqual(["dice", "echolings"]);
  });
});

describe("BrowseTab", () => {
  it("lists the plugins with Echolings highlighted at the top and nothing installed", async () => {
    withIndex();
    const { container } = render(<BrowseTab />, { wrapper: Wrapper });
    const list = await screen.findByRole("list", { name: "Available plugins" });
    const first = within(list).getAllByRole("listitem").at(0);
    expect(first?.querySelector("h3")?.textContent).toBe("Echolings");
    expect(first?.textContent).toContain("Featured");
    expect(screen.getAllByRole("button", { name: /^Install / })).toHaveLength(3);
    expect(screen.getAllByText("Official")).toHaveLength(3);
    expect(await axeViolations(container)).toEqual([]);
  });

  it("filters by category and search", async () => {
    withIndex();
    render(<BrowseTab />, { wrapper: Wrapper });
    fireEvent.click(await screen.findByRole("button", { name: "Games" }));
    expect(screen.getByRole("button", { name: "Games" }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getAllByRole("heading").map((h) => h.textContent)).toEqual(["Dice"]);
    fireEvent.click(screen.getByRole("button", { name: "All" }));
    fireEvent.change(screen.getByRole("searchbox", { name: "Search plugins" }), {
      target: { value: "nothing like this" },
    });
    expect(screen.getByText("No plugins match your search.")).toBeTruthy();
  });

  it("shows the offline notice and the never-fetched error with a retry", async () => {
    withIndex([dice], {
      freshness: "offline",
      notice: "Showing the last known plugin list (offline)",
    });
    const first = render(<BrowseTab />, { wrapper: Wrapper });
    expect(await screen.findByText("Showing the last known plugin list (offline)")).toBeTruthy();
    first.unmount();

    resetPluginIndex();
    const load = vi
      .spyOn(api, "pluginsIndex")
      .mockRejectedValue(
        new EngineError("serverUnreachable", "Can't load the plugin list. Check your connection."),
      );
    render(<BrowseTab />, { wrapper: Wrapper });
    expect((await screen.findByRole("alert")).textContent).toBe(
      "Can't load the plugin list. Check your connection.",
    );
    const before = load.mock.calls.length;
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => {
      expect(load.mock.calls.length).toBe(before + 1);
    });
  });

  it("marks installed plugins", async () => {
    withIndex([dice]);
    vi.spyOn(api, "pluginsList").mockResolvedValue({
      noPlugins: false,
      allOff: false,
      plugins: [{ id: "dice", source: { kind: "official" } }],
    } as never);
    render(<BrowseTab />, { wrapper: Wrapper });
    expect(await screen.findByText("Installed")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Install Dice" })).toBeNull();
  });

  it("opens the details of a requested official plugin and ignores unknown ids", async () => {
    withIndex();
    routePluginRequest({ kind: "details", plugin: "someone-elses" });
    render(<BrowseTab />, { wrapper: Wrapper });
    await screen.findByRole("list", { name: "Available plugins" });

    routePluginRequest({ kind: "details", plugin: "dice" });
    expect(await screen.findByRole("heading", { name: "Dice" })).toBeTruthy();
    expect(screen.getByText(/Version 1\.0\.0/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Back to all plugins" }));
    expect(await screen.findByRole("list", { name: "Available plugins" })).toBeTruthy();
  });

  it("opens the install prompt for an install request", async () => {
    withIndex();
    const prepare = vi.spyOn(api, "pluginInstallPrepare").mockReturnValue(new Promise(() => {}));
    routePluginRequest({ kind: "install", plugin: "poll" });
    render(<BrowseTab />, { wrapper: Wrapper });
    expect(await screen.findByRole("dialog", { name: "Install Poll?" })).toBeTruthy();
    expect(prepare).toHaveBeenCalledWith("poll");
  });

  it("previews permissions with a high-risk text in the details", async () => {
    withIndex([indexed("fetcher", { permissions: ["net.http", "ui.panel"] })]);
    render(<BrowseTab />, { wrapper: Wrapper });
    fireEvent.click(await screen.findByRole("button", { name: "Details of Fetcher" }));
    expect(screen.getByText("High risk")).toBeTruthy();
  });

  it("loads the icon through the engine and falls back to the initial", async () => {
    withIndex([dice, indexed("poll", { icon: "https://example.org/p.png" })]);
    const image = vi
      .spyOn(api, "pluginIndexImage")
      .mockResolvedValue("data:image/png;base64,iVBORw0KGgo=");
    const { container } = render(<BrowseTab />, { wrapper: Wrapper });
    await waitFor(() => {
      expect(container.querySelector("img")?.getAttribute("src")).toBe(
        "data:image/png;base64,iVBORw0KGgo=",
      );
    });
    expect(image).toHaveBeenCalledWith("poll", { kind: "icon" });
    expect(container.querySelector(".plugin-browse-initial")?.textContent).toBe("D");
  });
});

function DeepLinks() {
  usePluginDeepLinks();
  const pending = usePendingPluginRequest("details");
  return <p data-testid="pending">{pending?.plugin ?? "none"}</p>;
}

describe("usePluginDeepLinks", () => {
  it("opens official plugins from links and ignores other ids", async () => {
    withIndex([dice]);
    const opener = vi.fn();
    setSettingsOpener(opener);
    vi.spyOn(api, "pluginsTakePendingLink").mockResolvedValue("not-listed");
    render(<DeepLinks />, { wrapper: Wrapper });
    await waitFor(() => {
      expect(api.pluginsIndex).toHaveBeenCalled();
    });
    expect(opener).not.toHaveBeenCalled();

    api.mock.emit("pluginsOpenDetails", { id: "dice" });
    await waitFor(() => {
      expect(screen.getByTestId("pending").textContent).toBe("dice");
    });
    expect(opener).toHaveBeenCalledWith("plugins");
  });
});
