import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import {
  currentVersion,
  filterChangelog,
  itemHtml,
  summaryOf,
  visibleVersions,
  whatsNewOnStart,
  type ChangelogVersion,
} from "./changelog";
import { ChangelogView } from "./ChangelogView";
import { ChangelogWindow } from "./ChangelogWindow";
import { WhatsNewOnStart } from "./WhatsNewOnStart";

let api: MockApi;

beforeEach(async () => {
  await applyLanguage("en");
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  setApi(undefined);
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const versions: ChangelogVersion[] = [
  {
    version: "0.4.4",
    date: null,
    unreleased: true,
    sections: [
      { title: "Added", items: ["Code blocks have a **Copy** button (#111)"] },
      { title: "Internal", items: ["Faster CI"] },
    ],
  },
  {
    version: "0.4.3",
    date: "2026-10-28",
    unreleased: false,
    url: "https://github.com/Mumcord/mumcord/releases/tag/v0.4.3",
    sections: [
      { title: "Added", items: ["Message cards. They look nice.", "A packet inspector"] },
      { title: "Fixed", items: ["Echo on Windows"] },
      { title: "Internal", items: ["New lint rules"] },
    ],
  },
  {
    version: "0.4.0",
    date: "2026-10-26",
    unreleased: false,
    summary: "_Watch party is **Beta**._",
    sections: [{ title: "Multi-server", items: ["Several servers at once"] }],
  },
];

describe("changelog helpers", () => {
  it("shows versions in development only in dev builds", () => {
    expect(visibleVersions(versions, "0.4.3").map((v) => v.version)).toEqual(["0.4.3", "0.4.0"]);
    expect(visibleVersions(versions, "dev~a1b2c3d")).toHaveLength(3);
  });

  it("marks the running release, or the newest entry in a dev build, as current", () => {
    expect(currentVersion(versions, "0.4.0")).toBe("0.4.0");
    expect(currentVersion(versions, "dev~a1b2c3d")).toBe("0.4.4");
    expect(currentVersion(versions, "0.9.0-rc.1")).toBe("0.4.4");
    expect(currentVersion([], "0.4.3")).toBeNull();
  });

  it("summarises a version by its paragraph or its first entry's first sentence", () => {
    expect(summaryOf(versions[2] as ChangelogVersion)).toBe("Watch party is Beta.");
    expect(summaryOf(versions[1] as ChangelogVersion)).toBe("Message cards.");
    expect(summaryOf({ version: "1", date: null, unreleased: false, sections: [] })).toBeNull();
  });

  it("searches all versions, keeping only the matching entries", () => {
    expect(filterChangelog(versions, "  ")).toHaveLength(3);
    const found = filterChangelog(versions, "ECHO");
    expect(found).toEqual([
      { ...versions[1], sections: [{ title: "Fixed", items: ["Echo on Windows"] }] },
    ]);
    expect(filterChangelog(versions, "0.4.0").map((v) => v.version)).toEqual(["0.4.0"]);
    expect(filterChangelog(versions, "beta").map((v) => v.version)).toEqual(["0.4.0"]);
  });

  it("renders entries safely and links issue numbers", () => {
    expect(itemHtml("Fixed (issue #135)")).toContain(
      '<a href="https://github.com/Mumcord/mumcord/issues/135" rel="noopener noreferrer">#135</a>',
    );
    expect(itemHtml("C#12 and `#7` stay text")).not.toContain("<a");
    const html = itemHtml('<img src=x onerror="alert(1)"> [x](javascript:alert(1))');
    expect(html).not.toContain("<img");
    expect(html).not.toContain("javascript:");
  });

  it("opens What's new once per release, never for dev builds or a first start", () => {
    const state = {
      version: "0.4.4",
      wizardCompleted: true,
      whatsNewAuto: true,
      whatsNewSeen: "0.4.3",
    };
    expect(whatsNewOnStart(state)).toEqual({ open: true, remember: true });
    // Updated from a version that did not remember anything yet.
    expect(whatsNewOnStart({ ...state, whatsNewSeen: null })).toEqual({
      open: true,
      remember: true,
    });
    expect(whatsNewOnStart({ ...state, whatsNewSeen: "0.4.4" })).toEqual({
      open: false,
      remember: false,
    });
    expect(whatsNewOnStart({ ...state, whatsNewAuto: false })).toEqual({
      open: false,
      remember: true,
    });
    expect(whatsNewOnStart({ ...state, wizardCompleted: false, whatsNewSeen: null })).toEqual({
      open: false,
      remember: true,
    });
    expect(whatsNewOnStart({ ...state, version: "dev~a1b2c3d" })).toEqual({
      open: false,
      remember: false,
    });
  });
});

function renderView(appVersion = "0.4.3") {
  return render(
    <ChangelogView versions={versions} appVersion={appVersion} failed={false} onClose={vi.fn()} />,
  );
}

const toggle = (name: RegExp) => screen.getByRole("button", { name });

describe("ChangelogView", () => {
  it("opens the current release and collapses the older ones with date and summary", async () => {
    const { container } = renderView();
    expect(screen.getByRole("heading", { level: 1, name: "Mumcord 0.4.3" })).toBeVisible();
    expect(screen.getAllByRole("heading", { level: 2 })).toHaveLength(2);
    expect(screen.queryByText("0.4.4")).toBeNull();
    const current = toggle(/^0\.4\.3/u);
    expect(current).toHaveAttribute("aria-expanded", "true");
    expect(within(current).getByText("Current")).toBeVisible();
    expect(within(current).getByText("October 28, 2026")).toBeVisible();
    const older = toggle(/^0\.4\.0/u);
    expect(older).toHaveAttribute("aria-expanded", "false");
    expect(older).toHaveTextContent("Watch party is Beta.");
    expect(screen.getByRole("heading", { level: 3, name: /Added/u })).toHaveTextContent("Added2");
    expect(screen.getByText("A packet inspector")).toBeVisible();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("keeps Internal collapsed until opened", () => {
    renderView();
    const internal = screen.getByRole("button", { name: /Internal.*for developers/u });
    expect(internal).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("New lint rules")).toBeNull();
    fireEvent.click(internal);
    expect(internal).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("New lint rules")).toBeVisible();
  });

  it("expands and collapses versions one by one and all at once", () => {
    renderView();
    const older = toggle(/^0\.4\.0/u);
    fireEvent.click(older);
    expect(older).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("Several servers at once")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Collapse all" }));
    expect(toggle(/^0\.4\.3/u)).toHaveAttribute("aria-expanded", "false");
    expect(older).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(screen.getByRole("button", { name: "Expand all" }));
    expect(toggle(/^0\.4\.3/u)).toHaveAttribute("aria-expanded", "true");
    expect(older).toHaveAttribute("aria-expanded", "true");
  });

  it("moves between versions with the arrow keys, Home and End", () => {
    renderView("dev~a1b2c3d");
    const [first, second, third] = [toggle(/^0\.4\.4/u), toggle(/^0\.4\.3/u), toggle(/^0\.4\.0/u)];
    first.focus();
    fireEvent.keyDown(first, { key: "ArrowDown" });
    expect(second).toHaveFocus();
    fireEvent.keyDown(second, { key: "End" });
    expect(third).toHaveFocus();
    fireEvent.keyDown(third, { key: "Home" });
    expect(first).toHaveFocus();
  });

  it("marks the version in development in a dev build", () => {
    renderView("dev~a1b2c3d");
    const dev = toggle(/^0\.4\.4/u);
    expect(dev).toHaveAttribute("aria-expanded", "true");
    expect(within(dev).getByText("Current")).toBeVisible();
    expect(within(dev).getByText("In development")).toBeVisible();
    expect(toggle(/^0\.4\.3/u)).toHaveAttribute("aria-expanded", "false");
  });

  it("searches across versions and opens the matches", () => {
    renderView();
    fireEvent.change(screen.getByRole("searchbox", { name: "Search all versions" }), {
      target: { value: "servers" },
    });
    expect(screen.getAllByRole("heading", { level: 2 })).toHaveLength(1);
    expect(toggle(/^0\.4\.0/u)).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("status")).toHaveTextContent("Versions found: 1");
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "nothing like this" } });
    expect(screen.getByText("No entry matches the search.")).toBeVisible();
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "" } });
    expect(toggle(/^0\.4\.3/u)).toHaveAttribute("aria-expanded", "true");
    expect(toggle(/^0\.4\.0/u)).toHaveAttribute("aria-expanded", "false");
  });

  it("opens issue links and the release page through the link menu", async () => {
    const open = vi.spyOn(api, "openLink");
    renderView("dev~a1b2c3d");
    fireEvent.click(screen.getByRole("link", { name: "#111" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Open link" }));
    await waitFor(() => {
      expect(open).toHaveBeenCalledWith("https://github.com/Mumcord/mumcord/issues/111");
    });
    fireEvent.click(toggle(/^0\.4\.3/u));
    fireEvent.click(screen.getByRole("button", { name: "Release page" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Open link" }));
    await waitFor(() => {
      expect(open).toHaveBeenLastCalledWith(
        "https://github.com/Mumcord/mumcord/releases/tag/v0.4.3",
      );
    });
  });

  it("reports a changelog that failed to load", () => {
    render(<ChangelogView versions={null} appVersion={null} failed onClose={vi.fn()} />);
    expect(screen.getByRole("alert")).toHaveTextContent("The changelog could not be loaded.");
  });
});

describe("ChangelogWindow", () => {
  it("loads the changelog, follows the shared look and closes", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() => Promise.resolve(new Response(JSON.stringify(versions)))),
    );
    const close = vi.spyOn(api, "changelogClose");
    render(<ChangelogWindow />);
    expect(await screen.findByRole("heading", { level: 1, name: /^Mumcord/u })).toBeVisible();
    expect(await screen.findByRole("button", { name: /^0\.4\.3/u })).toBeVisible();

    await act(async () => {
      await api.windowLookShare({
        attributes: { theme: "dark" },
        properties: {},
        language: "de",
      });
    });
    await waitFor(() => {
      expect(document.documentElement.dataset.theme).toBe("dark");
    });
    expect(await screen.findByRole("button", { name: "Alle aufklappen" })).toBeVisible();
    expect(screen.getByText(/Die Änderungsliste ist auf Englisch\./u)).toBeVisible();

    fireEvent.keyDown(window, { key: "w", ctrlKey: true });
    expect(close).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Schließen" }));
    expect(close).toHaveBeenCalledTimes(2);
  });

  it("reports a changelog that cannot be fetched", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() => Promise.resolve(new Response("", { status: 404 }))),
    );
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    render(<ChangelogWindow />);
    expect(await screen.findByRole("alert")).toHaveTextContent("could not be loaded");
  });
});

describe("WhatsNewOnStart", () => {
  async function start(
    version: string,
    settings: Partial<Awaited<ReturnType<MockApi["settingsGet"]>>>,
  ) {
    const current = await api.settingsGet();
    vi.spyOn(api, "settingsGet").mockResolvedValue({ ...current, ...settings });
    vi.spyOn(api, "appInfo").mockResolvedValue({
      version,
      packageVersion: "0.4.4",
      os: "mock",
      appimage: false,
      distro: null,
    });
    const open = vi.spyOn(api, "changelogOpen");
    const seen = vi.spyOn(api, "whatsNewMarkSeen");
    render(<WhatsNewOnStart />);
    return { open, seen };
  }

  it("opens the window after an update and remembers the version", async () => {
    const { open, seen } = await start("0.4.4", { wizardCompleted: true, whatsNewSeen: "0.4.3" });
    await waitFor(() => {
      expect(open).toHaveBeenCalled();
    });
    expect(seen).toHaveBeenCalled();
  });

  it("stays closed in a dev build", async () => {
    const { open, seen } = await start("dev~a1b2c3d", { wizardCompleted: true });
    await act(async () => {
      await Promise.resolve();
    });
    expect(open).not.toHaveBeenCalled();
    expect(seen).not.toHaveBeenCalled();
  });
});
