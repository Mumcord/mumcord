import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createMockApi,
  setApi,
  type IndexedPlugin,
  type InstalledPlugin,
  type MockApi,
} from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { index, indexed } from "../plugins/browse/fixtures";
import { resetPluginIndex } from "../plugins/browse/indexStore";
import { routePluginRequest } from "../plugins/pluginRequestStore";
import { resetPluginStore } from "../plugins/pluginStore";
import { clearDismissedPreviews } from "./dismissedPreviews";
import { clearPreviews } from "./links";
import { LinkPreviews } from "./LinkPreviewCard";
import { PluginLinkCard } from "./PluginLinkCard";
import { parsePluginLink, pluginLinks } from "./pluginLink";

vi.mock("../plugins/pluginRequestStore", () => ({ routePluginRequest: vi.fn() }));

const LINK = "mumcord://plugins/whiteboard";

const WHITEBOARD = indexed("whiteboard", {
  summary: "Draw and sketch together on a shared canvas.",
});

function installed(id: string): InstalledPlugin {
  return {
    id,
    name: "Whiteboard",
    version: "1.0.0",
    source: { kind: "official" },
    status: { kind: "running" },
    enabled: true,
    disabledReason: null,
    required: [],
    optional: [],
    granted: [],
    visibility: "everyone",
    pinned: false,
    notifications: true,
    installedAt: null,
    badge: false,
    revocation: null,
    icon: null,
    description: "",
    authors: [],
    chatPatterns: [],
    offered: [],
    appMenuItems: 0,
    webSize: null,
  };
}

let api: MockApi;

function withIndex(plugins: IndexedPlugin[]): void {
  vi.spyOn(api, "pluginsIndex").mockResolvedValue(index(plugins));
}

function withInstalled(plugins: InstalledPlugin[]): void {
  vi.spyOn(api, "pluginsList").mockResolvedValue({
    noPlugins: false,
    allOff: false,
    plugins,
    refused: [],
  });
}

function renderCard(id = "whiteboard") {
  return render(
    <main>
      <PluginLinkCard id={id} tabIndex={0} />
    </main>,
  );
}

function renderPreviews(body: string, linkPreviews: boolean) {
  return render(
    <main>
      <LinkPreviews
        messageId="m1"
        dismissed={[]}
        html={body}
        body={body}
        linkPreviews={linkPreviews}
        tabIndex={0}
        onCopy={() => undefined}
      />
    </main>,
  );
}

/** Resolves once the card has left its loading state. */
async function settled(): Promise<void> {
  await waitFor(() => {
    expect(document.querySelector("[aria-busy='true']")).toBeNull();
  });
}

beforeAll(async () => {
  await applyLanguage("en");
});

beforeEach(() => {
  clearPreviews();
  clearDismissedPreviews();
  resetPluginIndex();
  resetPluginStore();
  vi.mocked(routePluginRequest).mockClear();
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("parsePluginLink", () => {
  it("reads the id of a plugin link", () => {
    expect(parsePluginLink(LINK)).toBe("whiteboard");
    expect(parsePluginLink("mumcord://plugins/abc")).toBe("abc");
    expect(parsePluginLink("mumcord://plugins/a2-b")).toBe("a2-b");
    expect(parsePluginLink(`mumcord://plugins/${"a".repeat(32)}`)).toBe("a".repeat(32));
  });

  it("accepts what the app's link handler accepts: any case of the scheme, a query, a fragment", () => {
    expect(parsePluginLink("MUMCORD://plugins/whiteboard")).toBe("whiteboard");
    expect(parsePluginLink("mumcord://plugins/whiteboard?from=chat")).toBe("whiteboard");
    expect(parsePluginLink("mumcord://plugins/whiteboard#top")).toBe("whiteboard");
  });

  it("refuses ids that are too short, too long, upper case or start wrong", () => {
    expect(parsePluginLink("mumcord://plugins/ab")).toBeNull();
    expect(parsePluginLink(`mumcord://plugins/${"a".repeat(33)}`)).toBeNull();
    expect(parsePluginLink("mumcord://plugins/Whiteboard")).toBeNull();
    expect(parsePluginLink("mumcord://plugins/1board")).toBeNull();
    expect(parsePluginLink("mumcord://plugins/-board")).toBeNull();
    expect(parsePluginLink("mumcord://plugins/white_board")).toBeNull();
    expect(parsePluginLink("mumcord://plugins/white%62oard")).toBeNull();
    expect(parsePluginLink("mumcord://plugins/")).toBeNull();
    expect(parsePluginLink("mumcord://plugins")).toBeNull();
  });

  it("refuses an extra path, another host and another scheme", () => {
    expect(parsePluginLink("mumcord://plugins/whiteboard/")).toBeNull();
    expect(parsePluginLink("mumcord://plugins/whiteboard/install")).toBeNull();
    expect(parsePluginLink("mumcord://plugins//whiteboard")).toBeNull();
    expect(parsePluginLink("mumcord://Plugins/whiteboard")).toBeNull();
    expect(parsePluginLink("mumcord://themes/whiteboard")).toBeNull();
    expect(parsePluginLink("mumcord:plugins/whiteboard")).toBeNull();
    expect(parsePluginLink("https://plugins/whiteboard")).toBeNull();
    expect(parsePluginLink("mumble://plugins/whiteboard")).toBeNull();
    expect(parsePluginLink("not a link")).toBeNull();
  });
});

describe("pluginLinks", () => {
  it("finds typed links and anchors in document order", () => {
    const html = `Try ${LINK} and <a href="mumcord://plugins/echolings">this</a>`;
    expect(pluginLinks(html)).toEqual([LINK, "mumcord://plugins/echolings"]);
  });

  it("lists a plugin once, however often and however it is linked", () => {
    const html = `${LINK} ${LINK}?a=1 <a href="${LINK}#x">again</a>`;
    expect(pluginLinks(html)).toEqual([LINK]);
  });

  it("leaves the sentence's punctuation out of a typed link", () => {
    expect(pluginLinks(`Have you seen ${LINK}?`)).toEqual([LINK]);
    expect(pluginLinks(`(${LINK}).`)).toEqual([LINK]);
  });

  it("skips links that are not plugin links", () => {
    const html = "mumcord://plugins/No mumcord://themes/dark https://example.org/whiteboard";
    expect(pluginLinks(html)).toEqual([]);
  });

  it("looks at no more than twenty links", () => {
    const html = Array.from({ length: 25 }, (_, n) => `mumcord://plugins/plugin-${String(n)}`).join(" ");
    expect(pluginLinks(html)).toHaveLength(20);
  });
});

describe("PluginLinkCard", () => {
  it("offers Install and Details for a plugin that is not installed", async () => {
    withIndex([WHITEBOARD]);
    const { container } = renderCard();
    const card = await screen.findByRole("article", { name: "Plugin card for Whiteboard" });
    expect(card.textContent).toContain("Official");
    expect(card.textContent).toContain("Draw and sketch together on a shared canvas.");
    expect(card.textContent).toContain("by Mumcord");
    expect(card.textContent).not.toContain("Installed");
    expect(screen.queryByRole("button", { name: "Open Whiteboard" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Install Whiteboard" }));
    expect(routePluginRequest).toHaveBeenLastCalledWith({ kind: "install", plugin: "whiteboard" });
    fireEvent.click(screen.getByRole("button", { name: "Details of Whiteboard" }));
    expect(routePluginRequest).toHaveBeenLastCalledWith({ kind: "details", plugin: "whiteboard" });
    expect(await axeViolations(container)).toEqual([]);
  });

  it("marks an installed plugin and offers Open and Details", async () => {
    withIndex([WHITEBOARD]);
    withInstalled([installed("whiteboard")]);
    const { container } = renderCard();
    const card = await screen.findByRole("article", { name: "Plugin card for Whiteboard" });
    expect(card.textContent).toContain("✓ Installed");
    expect(screen.queryByRole("button", { name: "Install Whiteboard" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Open Whiteboard" }));
    expect(routePluginRequest).toHaveBeenLastCalledWith({
      kind: "installed",
      plugin: "whiteboard",
    });
    fireEvent.click(screen.getByRole("button", { name: "Details of Whiteboard" }));
    expect(routePluginRequest).toHaveBeenLastCalledWith({ kind: "details", plugin: "whiteboard" });
    expect(await axeViolations(container)).toEqual([]);
  });

  it("shows the plugin's icon when it has one, and its initial otherwise", async () => {
    withIndex([indexed("whiteboard", { icon: "https://example.org/w.png" }), indexed("echolings")]);
    const image = vi
      .spyOn(api, "pluginIndexImage")
      .mockResolvedValue("data:image/png;base64,iVBORw0KGgo=");
    const withIcon = renderCard();
    await waitFor(() => {
      expect(withIcon.container.querySelector("img")?.getAttribute("src")).toBe(
        "data:image/png;base64,iVBORw0KGgo=",
      );
    });
    expect(image).toHaveBeenCalledWith("whiteboard", { kind: "icon" });
    withIcon.unmount();
    const withoutIcon = renderCard("echolings");
    await waitFor(() => {
      expect(withoutIcon.container.querySelector(".plugin-browse-initial")).not.toBeNull();
    });
    expect(withoutIcon.container.querySelector("img")).toBeNull();
  });

  it("does not load the plugin list again when a card mounts later", async () => {
    const load = vi.spyOn(api, "pluginsIndex").mockResolvedValue(index([indexed("whiteboard")]));
    const first = renderCard();
    await screen.findByRole("button", { name: "Details of Whiteboard" });
    first.unmount();
    renderCard();
    await screen.findByRole("button", { name: "Details of Whiteboard" });
    expect(load).toHaveBeenCalledTimes(1);
  });

  it("is a busy placeholder without buttons while the plugin list loads", async () => {
    vi.spyOn(api, "pluginsIndex").mockReturnValue(new Promise(() => undefined));
    const { container } = renderCard();
    const card = screen.getByRole("article", { name: "Loading plugin…" });
    expect(card.getAttribute("aria-busy")).toBe("true");
    expect(screen.queryByRole("button")).toBeNull();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("shows nothing for an id the plugin list does not know", async () => {
    withIndex([WHITEBOARD]);
    renderCard("does-not-exist");
    await settled();
    expect(screen.queryByRole("article")).toBeNull();
  });

  it("shows nothing for a plugin that is not official", async () => {
    withIndex([
      indexed("whiteboard", {
        source: { kind: "repo", repositoryId: 1, name: "Foo Repo" },
      }),
    ]);
    renderCard();
    await settled();
    expect(screen.queryByRole("article")).toBeNull();
  });

  it("shows nothing when the plugin list cannot be loaded", async () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    vi.spyOn(api, "pluginsIndex").mockRejectedValue(new Error("offline"));
    renderCard();
    await settled();
    expect(screen.queryByRole("article")).toBeNull();
  });

  it("renders the plugin list's texts as text, never as HTML", async () => {
    withIndex([indexed("whiteboard", { name: "<b>Board</b>", summary: "<img src=x>" })]);
    renderCard();
    const card = await screen.findByRole("article", { name: "Plugin card for <b>Board</b>" });
    expect(card.querySelector("b, img")).toBeNull();
  });

  it("hands its tab stop to every button", async () => {
    withIndex([WHITEBOARD]);
    render(
      <main>
        <PluginLinkCard id="whiteboard" tabIndex={-1} onDismiss={() => undefined} />
      </main>,
    );
    await screen.findByRole("article", { name: "Plugin card for Whiteboard" });
    const buttons = screen.getAllByRole("button");
    expect(buttons).toHaveLength(3);
    for (const button of buttons) expect(button.tabIndex).toBe(-1);
  });
});

describe("LinkPreviews", () => {
  it("shows the plugin card with link previews switched off", async () => {
    withIndex([WHITEBOARD]);
    const preview = vi.spyOn(api, "linkPreview");
    renderPreviews(`Try ${LINK} or https://news.example/story`, false);
    expect(await screen.findByRole("article", { name: "Plugin card for Whiteboard" })).toBeTruthy();
    expect(preview).not.toHaveBeenCalled();
  });

  it("renders no card for an unknown id", async () => {
    withIndex([WHITEBOARD]);
    renderPreviews("Try mumcord://plugins/does-not-exist", true);
    await settled();
    expect(screen.queryByRole("article")).toBeNull();
  });

  it("gives plugin links without a card no preview slot", async () => {
    withIndex([WHITEBOARD]);
    const preview = vi.spyOn(api, "linkPreview");
    renderPreviews(
      'mumcord://plugins/fake-one mumcord://plugins/fake-two mumcord://plugins/fake-three <a href="https://news.example/story">story</a>',
      true,
    );
    await waitFor(() => {
      expect(preview).toHaveBeenCalledWith("https://news.example/story");
    });
    expect(screen.queryByRole("article", { name: "Loading plugin…" })).toBeNull();
  });

  it("hides a dismissed card", async () => {
    withIndex([WHITEBOARD]);
    renderPreviews(`Try ${LINK}`, true);
    await screen.findByRole("article", { name: "Plugin card for Whiteboard" });
    fireEvent.click(screen.getByRole("button", { name: "Hide preview" }));
    await waitFor(() => {
      expect(screen.queryByRole("article")).toBeNull();
    });
  });
});
