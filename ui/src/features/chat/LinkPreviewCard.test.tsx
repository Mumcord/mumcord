import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
import { clearDismissedPreviews } from "./dismissedPreviews";
import { appPage } from "./embedWindow.testState";
import { cardPlayer, clearPreviews, previewableLinks } from "./links";
import { MessageList } from "./MessageList";

// Which page Mumcord's own window is: a website page (the development server, Windows) or the
// app's own scheme (macOS and Linux). Twitch's player only runs under a website page (#419).
vi.mock("./embedWindow", async () => {
  const { appPage } = await import("./embedWindow.testState");
  return { appPageIsWebPage: () => appPage.web };
});

const IMAGE = "data:image/jpeg;base64,/9j/4AAQ";

const PAGE: LinkPreview = {
  url: "https://news.example/story",
  siteName: "Example News",
  title: "A <b>headline</b>",
  description: "What happened today.",
  image: IMAGE,
  video: null,
  music: null,
  isImage: false,
  isAudio: false,
};

const YOUTUBE: VideoEmbed = {
  site: "youtube",
  playerUrl: "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ?autoplay=1",
  channel: "Rick",
};

const SPOTIFY: LinkPreview = {
  url: "https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC?si=abc",
  siteName: "Spotify",
  title: "Some Song",
  description: "Some Artist · Some Album · Song · 2020",
  image: IMAGE,
  video: null,
  music: {
    service: "spotify",
    kind: "track",
    playerUrl: "https://open.spotify.com/embed/track/4uLU6hMCjMI75M1A2tKUQC",
    artist: null,
  },
  isImage: false,
  isAudio: false,
};

const VIDEO: LinkPreview = {
  url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
  siteName: "YouTube",
  title: "Never gonna",
  description: null,
  image: IMAGE,
  video: YOUTUBE,
  music: null,
  isImage: false,
  isAudio: false,
};

function message(body: string, dismissedPreviews: string[] = []): ChatMessage {
  return {
    id: "1",
    conversation: { kind: "channel", channelId: 1 },
    senderName: "Maya",
    senderCertHash: null,
    senderSession: null,
    own: false,
    sentAt: new Date().toISOString(),
    body,
    dismissedPreviews,
    syncedFrom: null,
    file: null,
  };
}

function renderMessage(body: string, linkPreviews: boolean, dismissed: string[] = []) {
  return render(
    <main>
      <MessageList
        log={{
          messages: [message(body, dismissed)],
          loaded: true,
          loading: false,
          hasMore: false,
          loadFailed: false,
          syncing: false,
          syncResult: null,
          syncFailed: false,
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
  appPage.web = true;
  clearPreviews();
  clearDismissedPreviews();
  api = createMockApi();
  setApi(api);
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

const EMBED_PAGE = "http://localhost:43215/0123456789abcdef0123456789abcdef/embed.html";
const TWITCH = {
  site: "twitch" as const,
  playerUrl: "https://player.twitch.tv/?channel=someone&autoplay=true",
  channel: "someone",
};

describe("cardPlayer", () => {
  it("runs YouTube and Twitch in the loopback page", () => {
    expect(cardPlayer(YOUTUBE)).toEqual({ via: "page", url: YOUTUBE.playerUrl });
    expect(cardPlayer(TWITCH)).toEqual({ via: "page", url: TWITCH.playerUrl });
  });

  it("refuses URLs that are not a known player", () => {
    expect(
      cardPlayer({ ...YOUTUBE, playerUrl: "https://open.spotify.com/embed/track/4uLU6hMCjMI75" }),
    ).toBeNull();
    expect(cardPlayer({ ...YOUTUBE, playerUrl: "https://evil.example/embed" })).toBeNull();
    expect(cardPlayer({ ...YOUTUBE, playerUrl: "not a url" })).toBeNull();
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

    const openLink = vi.spyOn(api, "openLink").mockResolvedValue(null);
    fireEvent.click(screen.getByRole("button", { name: "Open A <b>headline</b> in browser" }));
    expect(openLink).toHaveBeenCalledWith(PAGE.url);
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

    const open = vi.spyOn(api, "embedPlayerOpen");
    const close = vi.spyOn(api, "embedPlayerClose");
    fireEvent.click(screen.getByRole("button", { name: "Play Never gonna" }));
    const frame = await screen.findByTitle<HTMLIFrameElement>("Video player: Never gonna");
    expect(open).toHaveBeenCalledTimes(1);
    expect(frame.getAttribute("src")).toBe(EMBED_PAGE);
    expect(frame.getAttribute("sandbox")).not.toContain("allow-popups");
    const target = frame.contentWindow;
    if (target === null) throw new Error("no frame window");
    const post = vi.spyOn(target, "postMessage").mockImplementation(() => undefined);
    fireEvent.load(frame);
    expect(post).toHaveBeenCalledWith(
      { type: "embed", url: YOUTUBE.playerUrl },
      "http://localhost:43215",
    );
    expect(screen.queryByRole("button", { name: "Play Never gonna" })).toBeNull();
    expect(screen.getByRole("button", { name: "Open Never gonna in browser" })).toBeVisible();

    cleanup();
    await waitFor(() => {
      expect(close).toHaveBeenCalledTimes(1);
    });
  });

  it("plays a Twitch card through the loopback page too", async () => {
    vi.spyOn(api, "linkPreview").mockResolvedValue({
      ...VIDEO,
      url: "https://www.twitch.tv/someone",
      siteName: "Twitch",
      title: "Live stream",
      video: TWITCH,
    });
    renderMessage(`<a href="https://www.twitch.tv/someone">stream</a>`, true);
    fireEvent.click(await screen.findByRole("button", { name: "Play Live stream" }));
    const frame = await screen.findByTitle<HTMLIFrameElement>("Video player: Live stream");
    expect(frame.getAttribute("src")).toBe(EMBED_PAGE);
    const target = frame.contentWindow;
    if (target === null) throw new Error("no frame window");
    const post = vi.spyOn(target, "postMessage").mockImplementation(() => undefined);
    fireEvent.load(frame);
    expect(post).toHaveBeenCalledWith(
      { type: "embed", url: TWITCH.playerUrl },
      "http://localhost:43215",
    );
  });

  // Issue #419: Twitch's player refuses to run under the app's own page unless that page is a
  // website, which `tauri://localhost` on Linux and macOS is not, so the card names that and hands
  // the stream to the browser instead of showing a picture that stays black.
  it("hands a Twitch card to the browser when the app's own page is no website", async () => {
    appPage.web = false;
    vi.spyOn(api, "linkPreview").mockResolvedValue({
      ...VIDEO,
      url: "https://www.twitch.tv/someone",
      siteName: "Twitch",
      title: "Live stream",
      video: TWITCH,
    });
    const openLink = vi.spyOn(api, "openLink").mockResolvedValue(null);
    const openPage = vi.spyOn(api, "embedPlayerOpen");
    const { container } = renderMessage('<a href="https://www.twitch.tv/someone">stream</a>', true);
    const play = await screen.findByRole("button", { name: "Play Live stream" });
    expect(
      await screen.findByText(/does not run inside Mumcord's window on this system/),
    ).toBeTruthy();

    fireEvent.click(play);
    await waitFor(() => {
      expect(openLink).toHaveBeenCalledWith("https://www.twitch.tv/someone");
    });
    expect(openPage).not.toHaveBeenCalled();
    expect(container.querySelector("iframe")).toBeNull();
    expect(await axeViolations(container)).toEqual([]);
  });

  // The Twitch card on the same overlay as the watch party (issue #163), with the card's own
  // full-screen control inside the element that goes full screen (issue #419). Twitch needs the
  // permission because its embed plays on the page of its own.
  it("keeps the Twitch card on the shared overlay and lets it go full screen", async () => {
    vi.spyOn(api, "linkPreview").mockResolvedValue({
      ...VIDEO,
      url: "https://www.twitch.tv/someone",
      siteName: "Twitch",
      title: "Live stream",
      video: TWITCH,
    });
    const { container } = renderMessage('<a href="https://www.twitch.tv/someone">stream</a>', true);
    const play = await screen.findByRole("button", { name: "Play Live stream" });
    const bar = play.closest(".media-overlay__bar");
    const box = play.closest(".media-overlay");
    if (bar === null || box === null) throw new Error("the Twitch card has no media overlay");
    expect(bar.contains(play)).toBe(true);
    expect(box).toHaveAttribute("data-paused");
    expect(await axeViolations(container)).toEqual([]);
    fireEvent.click(play);
    const frame = await screen.findByTitle<HTMLIFrameElement>("Video player: Live stream");
    expect(frame.getAttribute("allow")).toContain("fullscreen");
    expect(frame.hasAttribute("allowfullscreen")).toBe(true);
    const toolbar = box.querySelector("[role='toolbar']");
    if (toolbar === null) throw new Error("no toolbar in the Twitch card overlay");
    const fullScreen = screen.getByRole("button", { name: "Full screen" });
    expect(toolbar.contains(fullScreen)).toBe(true);
    expect(box).not.toHaveAttribute("data-paused");
    const request = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(box, "requestFullscreen", { value: request, configurable: true });
    fireEvent.click(fullScreen);
    expect(request).toHaveBeenCalledTimes(1);
  });

  it("copies the canonical video address with Mumcord's own control (issue #419)", async () => {
    const embed = "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ?autoplay=1";
    vi.spyOn(api, "linkPreview").mockResolvedValue({ ...VIDEO, url: embed });
    const copy = vi.spyOn(api, "copyText").mockResolvedValue(null);
    const { container } = renderMessage(`<a href="${embed}">video</a>`, true);
    await screen.findByRole("article", { name: "Link preview: Never gonna" });
    fireEvent.click(screen.getByRole("button", { name: "Copy link" }));
    expect(copy).toHaveBeenCalledWith("https://www.youtube.com/watch?v=dQw4w9WgXcQ");
    expect(await screen.findByText("Link copied")).toBeTruthy();
    expect(await axeViolations(container)).toEqual([]);
  });

  // Issue #419: the player's own button needs the permission, and the card's toolbar lives inside
  // the element that goes full screen, so a click can leave it as well.
  it("keeps the card toolbar inside the full-screen element and clicks it back out", async () => {
    vi.spyOn(api, "linkPreview").mockResolvedValue(VIDEO);
    renderMessage(`<a href="${VIDEO.url}">video</a>`, true);
    fireEvent.click(await screen.findByRole("button", { name: "Play Never gonna" }));
    const frame = await screen.findByTitle<HTMLIFrameElement>("Video player: Never gonna");
    expect(frame.getAttribute("allow")).toContain("fullscreen");
    expect(frame.hasAttribute("allowfullscreen")).toBe(true);
    const box = frame.closest(".link-card__player-box");
    const toolbar = box === null ? null : box.querySelector("[role='toolbar']");
    if (box === null || toolbar === null) throw new Error("no player box or toolbar");
    const fullScreen = screen.getByRole("button", { name: "Full screen" });
    expect(toolbar.contains(fullScreen)).toBe(true);
    const request = vi.fn().mockResolvedValue(undefined);
    const exit = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(box, "requestFullscreen", { value: request, configurable: true });
    Object.defineProperty(document, "exitFullscreen", { value: exit, configurable: true });
    fireEvent.click(fullScreen);
    expect(request).toHaveBeenCalledTimes(1);
    // The web view reports the state through an event; the same button then offers the way back.
    Object.defineProperty(document, "fullscreenElement", { value: box, configurable: true });
    document.dispatchEvent(new Event("fullscreenchange"));
    const leave = await waitFor(() => screen.getByRole("button", { name: "Leave full screen" }));
    expect(leave).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(leave);
    expect(exit).toHaveBeenCalledTimes(1);
  });

  // Issue #163: the card uses the same overlay as the watch party, so the two read as the same
  // control. The card counts as paused until it plays, which keeps its bar visible before the first
  // click; while a player runs the bar fades in on hover or keyboard focus.
  it("uses the watch party's media overlay for the card's controls", async () => {
    vi.spyOn(api, "linkPreview").mockResolvedValue(VIDEO);
    renderMessage(`<a href="${VIDEO.url}">video</a>`, true);
    const play = await screen.findByRole("button", { name: "Play Never gonna" });
    const bar = play.closest(".media-overlay__bar");
    const box = play.closest(".media-overlay");
    if (bar === null || box === null) throw new Error("the card has no media overlay");
    expect(bar.contains(play)).toBe(true);
    expect(box).toHaveAttribute("data-paused");
    expect(play.classList.contains("media-overlay__icon--main")).toBe(true);
    fireEvent.click(play);
    const frame = await screen.findByTitle<HTMLIFrameElement>("Video player: Never gonna");
    const running = frame.closest(".media-overlay");
    if (running === null) throw new Error("no overlay while the player runs");
    expect(running.hasAttribute("data-paused")).toBe(false);
  });

  it("opens the browser when the player page cannot be opened", async () => {
    vi.spyOn(api, "linkPreview").mockResolvedValue(VIDEO);
    vi.spyOn(api, "embedPlayerOpen").mockRejectedValue({ code: "io", message: "bind" });
    const opened = vi.spyOn(api, "openLink").mockResolvedValue(null);
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    renderMessage(`<a href="${VIDEO.url}">video</a>`, true);
    fireEvent.click(await screen.findByRole("button", { name: "Play Never gonna" }));
    await waitFor(() => {
      expect(opened).toHaveBeenCalledWith(VIDEO.url);
    });
    expect(screen.getByRole("button", { name: "Play Never gonna" })).toBeInTheDocument();
  });

  it("shows a music card for a music link and loads the player only after a click", async () => {
    vi.spyOn(api, "linkPreview").mockResolvedValue(SPOTIFY);
    const { container } = renderMessage(`<a href="${SPOTIFY.url}">song</a>`, true);
    const card = await screen.findByRole("article", { name: "Spotify: Some Song" });
    expect(card).toHaveTextContent("Some Artist · Some Album · Song · 2020");
    expect(container.querySelector("iframe")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Play Some Song on Spotify" }));
    const frame = await screen.findByTitle("Spotify player: Some Song");
    expect(frame.getAttribute("src")).toBe(SPOTIFY.music?.playerUrl);
  });

  it("shows the audio player for a direct audio link without asking for a preview", async () => {
    const url = "https://files.example.org/demos/night-drive.mp3";
    const preview = vi.spyOn(api, "linkPreview");
    const info = vi.spyOn(api, "audioInfo");
    const file = vi.spyOn(api, "audioFile");
    renderMessage(`<a href="${url}">${url}</a>`, true);
    await screen.findByRole("article", { name: "Audio file: night-drive.mp3" });
    expect(info).toHaveBeenCalledWith(url);
    expect(preview).not.toHaveBeenCalled();
    expect(file).not.toHaveBeenCalled();
  });

  it("shows the audio player for links that serve audio (by content type)", async () => {
    const url = "https://radio.example/stream?id=3";
    vi.spyOn(api, "linkPreview").mockResolvedValue({ ...PAGE, url, isAudio: true });
    vi.spyOn(api, "audioInfo").mockResolvedValue({ url, format: "ogg", size: null });
    renderMessage(`<a href="${url}">${url}</a>`, true);
    await screen.findByRole("article", { name: "Audio file: stream" });
  });

  it("shows plain links and requests nothing when previews are off", async () => {
    const lookup = vi.spyOn(api, "linkPreview");
    renderMessage(`<a href="${PAGE.url}">story</a>`, false);
    expect(screen.getByRole("link", { name: "story" })).toBeVisible();
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(lookup).not.toHaveBeenCalled();
    expect(screen.queryByRole("article")).toBeNull();
  });

  it("fetches nothing for audio and music links when previews are off", async () => {
    const lookup = vi.spyOn(api, "linkPreview");
    const info = vi.spyOn(api, "audioInfo");
    const audio = "https://files.example.org/night-drive.mp3";
    renderMessage(`<a href="${audio}">a</a> <a href="${SPOTIFY.url}">b</a>`, false);
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(lookup).not.toHaveBeenCalled();
    expect(info).not.toHaveBeenCalled();
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

describe("dismissing preview cards", () => {
  it("hides the card with the x button and tells the engine", async () => {
    vi.spyOn(api, "linkPreview").mockResolvedValue(PAGE);
    const dismiss = vi.spyOn(api, "previewDismiss");
    const { container } = renderMessage(`<a href="${PAGE.url}">story</a>`, true);
    await screen.findByRole("article");
    expect(await axeViolations(container)).toEqual([]);

    fireEvent.click(screen.getByRole("button", { name: "Hide preview" }));
    expect(dismiss).toHaveBeenCalledWith("1", PAGE.url);
    expect(screen.queryByRole("article")).toBeNull();
    expect(container.querySelector(".link-previews")).toBeNull();
    expect(screen.getByRole("link", { name: "story" })).toBeVisible();
  });

  it("hides only the dismissed card of a message with several links", async () => {
    vi.spyOn(api, "linkPreview").mockImplementation((url) =>
      Promise.resolve({ ...PAGE, url, title: url }),
    );
    renderMessage(`<a href="https://a.example/">a</a> <a href="https://b.example/">b</a>`, true);
    await waitFor(() => {
      expect(screen.getAllByRole("article")).toHaveLength(2);
    });
    fireEvent.click(screen.getAllByRole("button", { name: "Hide preview" })[0] as HTMLElement);
    const left = screen.getAllByRole("article");
    expect(left).toHaveLength(1);
    expect(left[0]).toHaveAccessibleName("Link preview: https://b.example/");
  });

  it("keeps cards hidden that were dismissed earlier, without loading them", async () => {
    const lookup = vi.spyOn(api, "linkPreview").mockResolvedValue(PAGE);
    renderMessage(`<a href="${PAGE.url}">story</a>`, true, [PAGE.url]);
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(screen.queryByRole("article")).toBeNull();
    expect(lookup).not.toHaveBeenCalled();
  });

  it("hides video cards too and survives a re-render of the list", async () => {
    vi.spyOn(api, "linkPreview").mockResolvedValue(VIDEO);
    const view = renderMessage(`<a href="${VIDEO.url}">video</a>`, true);
    await screen.findByRole("article", { name: "Link preview: Never gonna" });
    fireEvent.click(screen.getByRole("button", { name: "Hide preview" }));
    expect(screen.queryByRole("article")).toBeNull();
    view.unmount();
    renderMessage(`<a href="${VIDEO.url}">video</a>`, true);
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(screen.queryByRole("article")).toBeNull();
  });

  it("keeps a failed save to the engine from bringing the card back", async () => {
    vi.spyOn(api, "linkPreview").mockResolvedValue(PAGE);
    vi.spyOn(api, "previewDismiss").mockRejectedValue(new Error("disk full"));
    const log = vi.spyOn(console, "error").mockImplementation(() => undefined);
    renderMessage(`<a href="${PAGE.url}">story</a>`, true);
    await screen.findByRole("article");
    fireEvent.click(screen.getByRole("button", { name: "Hide preview" }));
    await waitFor(() => {
      expect(log).toHaveBeenCalled();
    });
    expect(screen.queryByRole("article")).toBeNull();
  });
});
