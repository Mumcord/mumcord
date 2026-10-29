import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { LicencesView } from "./LicencesView";
import { LicencesWindow } from "./LicencesWindow";
import {
  entryText,
  filterEntries,
  licenceOptions,
  type NoticeEntry,
  type Notices,
} from "./notices";
import { WindowLookShare } from "./WindowLookShare";

let api: MockApi;

// jsdom has no layout: the list gets a height and each row one, so the virtualizer renders the
// rows that fit like in the window.
const LIST_HEIGHT = 800;
const ROW_HEIGHT = 40;

beforeEach(async () => {
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockImplementation(function (
    this: HTMLElement,
  ) {
    if (this.classList.contains("lic-list")) return LIST_HEIGHT;
    return this.classList.contains("lic-row") ? ROW_HEIGHT : 0;
  });
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(800);
  await applyLanguage("en");
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  setApi(undefined);
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function entry(over: Partial<NoticeEntry>): NoticeEntry {
  return {
    name: "tokio",
    version: "1.47.1",
    license: "MIT",
    licenses: ["MIT"],
    repository: "https://github.com/tokio-rs/tokio",
    source: "rust",
    textIds: ["mit"],
    ...over,
  };
}

const notices: Notices = {
  entries: [
    entry({}),
    entry({
      name: "serde",
      version: "1.0.0",
      license: "MIT OR Apache-2.0",
      licenses: ["MIT", "Apache-2.0"],
      textIds: ["mit", "apache"],
    }),
    entry({
      name: "react",
      version: "19.3.1",
      source: "npm",
      repository: "https://github.com/facebook/react",
    }),
    entry({
      name: "GStreamer",
      version: "",
      license: "LGPL-2.1-or-later",
      licenses: ["LGPL-2.1-or-later"],
      source: "native",
      textIds: ["lgpl"],
      description: "Bundled in the AppImage.",
    }),
  ],
  texts: { mit: "MIT License text", apache: "Apache License text", lgpl: "LGPL text" },
};

const toggles = () => screen.getAllByRole("button", { expanded: false });

describe("licence list helpers", () => {
  it("searches name and licence, any case", () => {
    expect(filterEntries(notices.entries, "TOK", null).map((e) => e.name)).toEqual(["tokio"]);
    expect(filterEntries(notices.entries, "apache", null).map((e) => e.name)).toEqual(["serde"]);
    expect(filterEntries(notices.entries, "  ", null)).toHaveLength(4);
  });

  it("filters by licence id", () => {
    expect(filterEntries(notices.entries, "", "Apache-2.0").map((e) => e.name)).toEqual(["serde"]);
    expect(filterEntries(notices.entries, "o", "MIT").map((e) => e.name)).toEqual([
      "tokio",
      "serde",
    ]);
  });

  it("offers the licences most used first", () => {
    expect(licenceOptions(notices.entries)).toEqual([
      { id: "MIT", count: 3 },
      { id: "Apache-2.0", count: 1 },
      { id: "LGPL-2.1-or-later", count: 1 },
    ]);
  });

  it("joins several texts of one entry", () => {
    expect(entryText(notices.entries[1] as NoticeEntry, notices.texts)).toBe(
      "MIT License text\n\nApache License text",
    );
  });
});

describe("LicencesView", () => {
  it("shows Mumcord's own licence and one collapsed row per component", async () => {
    const { container } = render(<LicencesView notices={notices} failed={false} />);
    expect(screen.getByRole("heading", { level: 1, name: "Third-party licences" })).toBeVisible();
    expect(screen.getByText(/AGPL-3\.0-only/u)).toBeVisible();
    expect(screen.getByRole("button", { name: "Source code" })).toBeVisible();
    expect(screen.getAllByRole("heading", { level: 2 })).toHaveLength(4);
    const tokio = screen.getByRole("button", { name: /tokio 1\.47\.1/u });
    expect(tokio).toHaveAttribute("aria-expanded", "false");
    expect(tokio).toHaveTextContent("tokio 1.47.1— MIT");
    expect(within(tokio).getByText("Rust")).toBeVisible();
    expect(screen.getByRole("button", { name: /^GStreamer/u })).toHaveTextContent("Native");
    expect(screen.queryByText("MIT License text")).toBeNull();
    expect(screen.getByText("Shown: 4 of 4")).toBeVisible();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("expands a row by click to its text, repository and copy button", async () => {
    const copy = vi.spyOn(api, "copyText");
    render(<LicencesView notices={notices} failed={false} />);
    const serde = screen.getByRole("button", { name: /serde/u });
    fireEvent.click(serde);
    expect(serde).toHaveAttribute("aria-expanded", "true");
    const panel = document.getElementById(serde.getAttribute("aria-controls") ?? "");
    expect(panel).not.toBeNull();
    expect(panel).toBeVisible();
    const text = within(panel as HTMLElement).getByLabelText("Licence text of serde");
    expect(text.tagName).toBe("PRE");
    expect(text).toHaveTextContent("MIT License text Apache License text");
    fireEvent.click(
      within(panel as HTMLElement).getByRole("button", { name: "Copy licence text" }),
    );
    await waitFor(() => {
      expect(copy).toHaveBeenCalledWith("MIT License text\n\nApache License text");
    });

    fireEvent.click(serde);
    expect(serde).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText(/Apache License text/u)).toBeNull();
  });

  it("opens the repository through the link menu", async () => {
    const open = vi.spyOn(api, "openLink");
    render(<LicencesView notices={notices} failed={false} />);
    fireEvent.click(screen.getByRole("button", { name: /react/u }));
    fireEvent.click(screen.getByRole("button", { name: "https://github.com/facebook/react" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Open link" }));
    await waitFor(() => {
      expect(open).toHaveBeenCalledWith("https://github.com/facebook/react");
    });
  });

  it("shows a native component's description", () => {
    render(<LicencesView notices={notices} failed={false} />);
    fireEvent.click(screen.getByRole("button", { name: /GStreamer/u }));
    expect(screen.getByText("Bundled in the AppImage.")).toBeVisible();
    expect(screen.getByText("LGPL text")).toBeVisible();
  });

  it("filters by search and licence", async () => {
    render(<LicencesView notices={notices} failed={false} />);
    fireEvent.change(screen.getByRole("searchbox", { name: "Search by name or licence" }), {
      target: { value: "lgpl" },
    });
    expect(screen.getAllByRole("heading", { level: 2 })).toHaveLength(1);
    expect(screen.getByText("Shown: 1 of 4")).toBeVisible();
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "nothing-like-this" } });
    expect(screen.getByText("No component matches the search.")).toBeVisible();
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "" } });

    fireEvent.click(screen.getByRole("combobox", { name: "Licence" }));
    fireEvent.click(await screen.findByRole("option", { name: "Apache-2.0 (1)" }));
    expect(screen.getAllByRole("heading", { level: 2 })).toHaveLength(1);
    expect(screen.getByRole("button", { name: /serde/u })).toBeVisible();
  });

  it("expands and collapses all shown rows", () => {
    render(<LicencesView notices={notices} failed={false} />);
    expect(toggles()).toHaveLength(4);
    fireEvent.click(screen.getByRole("button", { name: "Expand all" }));
    expect(screen.getAllByRole("button", { expanded: true })).toHaveLength(4);
    expect(screen.getAllByText("MIT License text")).toHaveLength(2);
    fireEvent.click(screen.getByRole("button", { name: "Collapse all" }));
    expect(toggles()).toHaveLength(4);
  });

  it("moves between rows with the arrow keys, Home and End", async () => {
    render(<LicencesView notices={notices} failed={false} />);
    const rows = toggles();
    (rows[0] as HTMLElement).focus();
    fireEvent.keyDown(rows[0] as HTMLElement, { key: "ArrowDown" });
    await waitFor(() => {
      expect(rows[1]).toHaveFocus();
    });
    fireEvent.keyDown(rows[1] as HTMLElement, { key: "End" });
    await waitFor(() => {
      expect(rows[3]).toHaveFocus();
    });
    fireEvent.keyDown(rows[3] as HTMLElement, { key: "ArrowDown" });
    fireEvent.keyDown(rows[3] as HTMLElement, { key: "Home" });
    await waitFor(() => {
      expect(rows[0]).toHaveFocus();
    });
    fireEvent.keyDown(rows[0] as HTMLElement, { key: "ArrowUp" });
    expect(rows[0]).toHaveFocus();
  });

  it("renders only the rows in view of a long list", () => {
    const many: Notices = {
      entries: Array.from({ length: 500 }, (_, i) => entry({ name: `crate-${String(i)}` })),
      texts: notices.texts,
    };
    render(<LicencesView notices={many} failed={false} />);
    const rendered = screen.getAllByRole("heading", { level: 2 }).length;
    expect(rendered).toBeGreaterThan(0);
    expect(rendered).toBeLessThan(LIST_HEIGHT / ROW_HEIGHT + 20);
    expect(screen.getByText("Shown: 500 of 500")).toBeVisible();
  });

  it("reports a list that failed to load", () => {
    render(<LicencesView notices={null} failed />);
    expect(screen.getByRole("alert")).toHaveTextContent("The licence list could not be loaded.");
  });
});

describe("LicencesWindow", () => {
  it("loads the list, follows the shared look and closes on Ctrl+W", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() => Promise.resolve(new Response(JSON.stringify(notices)))),
    );
    const close = vi.spyOn(api, "licencesClose");
    render(<LicencesWindow />);
    expect(await screen.findByRole("button", { name: /tokio/u })).toBeVisible();

    await act(async () => {
      await api.windowLookShare({
        attributes: { theme: "dark" },
        properties: { "--color-bg": "#000000" },
        language: "de",
      });
    });
    await waitFor(() => {
      expect(document.documentElement.dataset.theme).toBe("dark");
    });
    expect(await screen.findByRole("heading", { name: "Drittanbieter-Lizenzen" })).toBeVisible();

    fireEvent.keyDown(window, { key: "w", ctrlKey: true });
    expect(close).toHaveBeenCalled();
  });

  it("reports a list that cannot be fetched", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() => Promise.resolve(new Response("", { status: 404 }))),
    );
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    render(<LicencesWindow />);
    expect(await screen.findByRole("alert")).toHaveTextContent("could not be loaded");
  });
});

describe("main window", () => {
  it("shares its look for the licences window", async () => {
    const share = vi.spyOn(api, "windowLookShare");
    document.documentElement.dataset.theme = "light";
    render(<WindowLookShare />);
    await waitFor(() => {
      expect(share.mock.lastCall?.[0].attributes.theme).toBe("light");
    });
  });
});
