import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createMockApi,
  setApi,
  type ChatMessage,
  type LinkPreview,
  type MockApi,
  type VideoEmbed,
} from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { clearPreviews, playerUrl, previewableLinks } from "./links";
import { MessageList } from "./MessageList";

const opener = vi.hoisted(() => ({ openUrl: vi.fn(() => Promise.resolve()) }));
vi.mock("@tauri-apps/plugin-opener", () => opener);

const IMAGE = "data:image/jpeg;base64,/9j/4AAQ";

const PAGE: LinkPreview = {
  url: "https://news.example/story",
  siteName: "Example News",
  title: "A <b>headline</b>",
  description: "What happened today.",
  image: IMAGE,
  video: null,
};

const YOUTUBE: VideoEmbed = {
  site: "youtube",
  playerUrl: "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ?autoplay=1",
  channel: "Rick",
};

const VIDEO: LinkPreview = {
  url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
  siteName: "YouTube",
  title: "Never gonna",
  description: null,
  image: IMAGE,
  video: YOUTUBE,
};

function message(body: string): ChatMessage {
  return {
    id: "1",
    conversation: { kind: "channel", channelId: 1 },
    senderName: "Maya",
    senderCertHash: null,
    senderSession: null,
    own: false,
    sentAt: new Date().toISOString(),
    body,
  };
}

function renderMessage(body: string, linkPreviews: boolean) {
  return render(
    <main>
      <MessageList
        log={{
          messages: [message(body)],
          loaded: true,
          loading: false,
          hasMore: false,
          loadFailed: false,
        }}
        label="Messages in #Lobby"
        onLoadOlder={() => undefined}
        linkPreviews={linkPreviews}
      />
    </main>,
  );
}

let api: MockApi;

beforeAll(async () => {
  await applyLanguage("en");
});

beforeEach(() => {
  clearPreviews();
  api = createMockApi();
  setApi(api);
  opener.openUrl.mockClear();
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
});

describe("previewableLinks", () => {
  it("takes distinct http(s) links in order, at most three", () => {
    const html =
      '<a href="https://a.example/">a</a> <a href="mailto:x@y.z">m</a> <a href="https://a.example/">again</a>' +
      '<a href="http://b.example/">b</a><a href="https://c.example/">c</a><a href="https://d.example/">d</a>';
    expect(previewableLinks(html)).toEqual([
      "https://a.example/",
      "http://b.example/",
      "https://c.example/",
    ]);
    expect(previewableLinks("<p>no links</p>")).toEqual([]);
  });
});

describe("playerUrl", () => {
  const twitch = {
    site: "twitch" as const,
    playerUrl: "https://player.twitch.tv/?channel=someone&autoplay=true",
    channel: "someone",
  };

  it("embeds YouTube on http(s) pages and Twitch with its parent where Twitch allows it", () => {
    const dev = { protocol: "http:", hostname: "localhost" };
    expect(playerUrl(YOUTUBE, dev)).toBe(YOUTUBE.playerUrl);
    expect(playerUrl(twitch, dev)).toBe(
      "https://player.twitch.tv/?channel=someone&autoplay=true&parent=localhost",
    );
    expect(playerUrl(twitch, { protocol: "https:", hostname: "tauri.localhost" })).toContain(
      "parent=tauri.localhost",
    );
  });

  it("falls back to the browser where the player cannot run", () => {
    const tauriScheme = { protocol: "tauri:", hostname: "localhost" };
    expect(playerUrl(YOUTUBE, tauriScheme)).toBeNull();
    expect(playerUrl(twitch, tauriScheme)).toBeNull();
    expect(playerUrl(twitch, { protocol: "http:", hostname: "tauri.localhost" })).toBeNull();
    const foreign = { ...YOUTUBE, playerUrl: "https://evil.example/embed" };
    expect(playerUrl(foreign, { protocol: "http:", hostname: "localhost" })).toBeNull();
  });
});

describe("link preview cards", () => {
  it("shows a page card with plain-text title, description, image and Open in browser", async () => {
    vi.spyOn(api, "linkPreview").mockResolvedValue(PAGE);
    const { container } = renderMessage(`<a href="${PAGE.url}">story</a>`, true);
    const card = await screen.findByRole("article", { name: "Link preview: A <b>headline</b>" });
    expect(card).toHaveTextContent("Example News");
    expect(card).toHaveTextContent("What happened today.");
    expect(card.querySelector("b")).toBeNull();
    expect(card.querySelector("img")?.getAttribute("src")).toBe(IMAGE);
    expect(api.linkPreview).toHaveBeenCalledWith(PAGE.url);

    const internals = window as unknown as Record<string, unknown>;
    internals.__TAURI_INTERNALS__ = {};
    try {
      fireEvent.click(screen.getByRole("button", { name: "Open A <b>headline</b> in browser" }));
      await waitFor(() => {
        expect(opener.openUrl).toHaveBeenCalledWith(PAGE.url);
      });
    } finally {
      delete internals.__TAURI_INTERNALS__;
    }
    expect(await axeViolations(container)).toEqual([]);
  });

  it("ignores images that are not engine data URLs", async () => {
    vi.spyOn(api, "linkPreview").mockResolvedValue({
      ...PAGE,
      image: "https://tracker.example/pixel.png",
    });
    renderMessage(`<a href="${PAGE.url}">story</a>`, true);
    const card = await screen.findByRole("article");
    expect(card.querySelector("img")).toBeNull();
  });

  it("loads the video player only after a click on play", async () => {
    vi.spyOn(api, "linkPreview").mockResolvedValue(VIDEO);
    const { container } = renderMessage(`<a href="${VIDEO.url}">video</a>`, true);
    const card = await screen.findByRole("article", { name: "Link preview: Never gonna" });
    expect(card).toHaveTextContent("Rick");
    expect(container.querySelector("iframe")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Play Never gonna" }));
    const frame = await screen.findByTitle("Video player: Never gonna");
    expect(frame.getAttribute("src")).toBe(YOUTUBE.playerUrl);
    expect(frame.getAttribute("sandbox")).not.toContain("allow-popups");
    expect(screen.queryByRole("button", { name: "Play Never gonna" })).toBeNull();
    expect(screen.getByRole("button", { name: "Open Never gonna in browser" })).toBeVisible();
  });

  it("shows plain links and requests nothing when previews are off", async () => {
    const lookup = vi.spyOn(api, "linkPreview");
    renderMessage(`<a href="${PAGE.url}">story</a>`, false);
    expect(screen.getByRole("link", { name: "story" })).toBeVisible();
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(lookup).not.toHaveBeenCalled();
    expect(screen.queryByRole("article")).toBeNull();
  });

  it("shows nothing extra when there is no preview", async () => {
    const lookup = vi.spyOn(api, "linkPreview").mockResolvedValue(null);
    renderMessage(`<a href="${PAGE.url}">story</a>`, true);
    await waitFor(() => {
      expect(lookup).toHaveBeenCalledOnce();
    });
    expect(screen.queryByRole("article")).toBeNull();
  });
});
