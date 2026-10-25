import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createMockApi,
  setApi,
  type ChatMessage,
  type InlineImage,
  type LinkPreview,
  type MockApi,
} from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { clearDismissedPreviews } from "./dismissedPreviews";
import { InlineImageCard } from "./InlineImageCard";
import { clearPreviews, externalImages, fitImage, IMAGE_BOX, isImageLink } from "./links";
import { MessageList } from "./MessageList";

const opener = vi.hoisted(() => ({ openUrl: vi.fn(() => Promise.resolve()) }));
vi.mock("@tauri-apps/plugin-opener", () => opener);

const REMOTE = "https://pics.example/cat.gif";
const GIF: InlineImage = {
  url: REMOTE,
  src: "data:image/gif;base64,R0lGODlhAQABAAAAACw=",
  width: 800,
  height: 400,
  animated: true,
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
    dismissedPreviews: [],
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
  clearDismissedPreviews();
  api = createMockApi();
  setApi(api);
  opener.openUrl.mockClear();
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
});

describe("image detection", () => {
  it("recognises direct image links by their extension", () => {
    expect(isImageLink("https://e.example/a/b.PNG")).toBe(true);
    expect(isImageLink("https://e.example/photo.jpeg?size=large")).toBe(true);
    expect(isImageLink("https://e.example/anim.webp#x")).toBe(true);
    expect(isImageLink("https://e.example/page.html")).toBe(false);
    expect(isImageLink("https://e.example/png")).toBe(false);
    expect(isImageLink("not a url")).toBe(false);
  });

  it("takes distinct http(s) image sources, never data URLs", () => {
    const html =
      `<img src="${REMOTE}"><img src="data:image/png;base64,AAAA"><img src="${REMOTE}">` +
      '<img src="http://b.example/x.png"><img>';
    expect(externalImages(html)).toEqual([REMOTE, "http://b.example/x.png"]);
  });

  it("fits images into 400×300 with the aspect ratio kept, never scaling up", () => {
    expect(fitImage(800, 400)).toEqual({ width: 400, height: 200 });
    expect(fitImage(300, 1200)).toEqual({ width: 75, height: 300 });
    expect(fitImage(120, 90)).toEqual({ width: 120, height: 90 });
  });
});

describe("inline images", () => {
  it("shows an external image inline from the engine's data URL, not the remote address", async () => {
    vi.spyOn(api, "inlineImage").mockResolvedValue(GIF);
    const { container } = renderMessage(`look <img src="${REMOTE}">`, true);
    const figure = await screen.findByRole("figure", { name: "Image: cat.gif" });
    const img = figure.querySelector("img");
    expect(img?.getAttribute("src")).toBe(GIF.src);
    expect(img?.getAttribute("width")).toBe("400");
    expect(img?.getAttribute("height")).toBe("200");
    expect(api.inlineImage).toHaveBeenCalledWith(REMOTE);
    expect(container.querySelector(`img[src="${REMOTE}"]`)).toBeNull();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("credits Giphy and Tenor GIFs with a link to the original page, and no one else", async () => {
    const media = "https://media2.giphy.com/media/1ojn1S7BTXUry8elNi/giphy.gif";
    vi.spyOn(api, "inlineImage").mockImplementation((url) => Promise.resolve({ ...GIF, url }));
    const giphy = renderMessage(`<img src="${media}">`, true);
    const link = await screen.findByRole("link", { name: "via GIPHY" });
    expect(link).toHaveTextContent("via GIPHY");
    const open = vi.spyOn(window, "open").mockReturnValue(null);
    fireEvent.click(link);
    expect(open).toHaveBeenCalledWith(
      "https://giphy.com/gifs/1ojn1S7BTXUry8elNi",
      "_blank",
      "noopener,noreferrer",
    );
    giphy.unmount();
    renderMessage('<img src="https://media.tenor.com/abc/cat.gif">', true);
    expect(await screen.findByRole("link", { name: "via Tenor" })).toHaveTextContent("via Tenor");
  });

  it("shows no credit on other images", async () => {
    vi.spyOn(api, "inlineImage").mockResolvedValue(GIF);
    renderMessage(`<img src="${REMOTE}">`, true);
    await screen.findByRole("figure", { name: "Image: cat.gif" });
    expect(screen.queryByRole("link", { name: /via / })).toBeNull();
  });

  it("holds a fixed-size placeholder while the image loads", () => {
    vi.spyOn(api, "inlineImage").mockReturnValue(new Promise(() => undefined));
    renderMessage(`<img src="${REMOTE}">`, true);
    const placeholder = screen.getByRole("img", { name: "Loading image cat.gif" });
    expect(placeholder).toHaveStyle({
      width: `${String(IMAGE_BOX.width)}px`,
      height: `${String(IMAGE_BOX.height)}px`,
    });
  });

  it("shows direct image links as images instead of cards", async () => {
    const png = "https://pics.example/dog.png";
    const preview = vi.spyOn(api, "linkPreview");
    vi.spyOn(api, "inlineImage").mockResolvedValue({ ...GIF, url: png, animated: false });
    renderMessage(`<a href="${png}">${png}</a>`, true);
    await screen.findByRole("figure", { name: "Image: dog.png" });
    expect(preview).not.toHaveBeenCalled();
  });

  it("shows links that serve an image (by content type) as images", async () => {
    const url = "https://pics.example/view?id=7";
    const flagged: LinkPreview = {
      url,
      siteName: null,
      title: null,
      description: null,
      image: null,
      video: null,
      isImage: true,
    };
    vi.spyOn(api, "linkPreview").mockResolvedValue(flagged);
    vi.spyOn(api, "inlineImage").mockResolvedValue({ ...GIF, url });
    renderMessage(`<a href="${url}">${url}</a>`, true);
    await screen.findByRole("figure", { name: "Image: view" });
    expect(screen.queryByRole("article")).toBeNull();
    expect(api.inlineImage).toHaveBeenCalledWith(url);
  });

  it("loads nothing and shows no image when previews are off", async () => {
    const lookup = vi.spyOn(api, "inlineImage");
    const { container } = renderMessage(
      `x <img src="${REMOTE}"> <a href="https://pics.example/dog.png">dog</a>`,
      false,
    );
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(lookup).not.toHaveBeenCalled();
    expect(container.querySelector("img")).toBeNull();
    expect(screen.queryByRole("figure")).toBeNull();
  });

  it("drops the box when the engine has no image, and ignores non-data sources", async () => {
    vi.spyOn(api, "inlineImage").mockResolvedValueOnce(null);
    renderMessage(`<img src="${REMOTE}">`, true);
    await waitFor(() => {
      expect(screen.queryByRole("img", { name: "Loading image cat.gif" })).toBeNull();
    });
    expect(screen.queryByRole("figure")).toBeNull();

    clearPreviews();
    vi.spyOn(api, "inlineImage").mockResolvedValue({ ...GIF, src: REMOTE });
    const other = render(<InlineImageCard url={REMOTE} messageId="2" tabIndex={0} />);
    await waitFor(() => {
      expect(other.container.querySelector(".inline-image--loading")).toBeNull();
    });
    expect(other.container.querySelector("img")).toBeNull();
  });

  it("opens the browser from Open in browser", async () => {
    vi.spyOn(api, "inlineImage").mockResolvedValue(GIF);
    const internals = window as unknown as Record<string, unknown>;
    internals.__TAURI_INTERNALS__ = {};
    try {
      renderMessage(`<img src="${REMOTE}">`, true);
      fireEvent.click(await screen.findByRole("button", { name: "Open cat.gif in browser" }));
      await waitFor(() => {
        expect(opener.openUrl).toHaveBeenCalledWith(REMOTE);
      });
    } finally {
      delete internals.__TAURI_INTERNALS__;
    }
  });

  it("hands a click to onImageClick (the image viewer) instead of the browser", async () => {
    vi.spyOn(api, "inlineImage").mockResolvedValue(GIF);
    const onImageClick = vi.fn();
    render(<InlineImageCard url={REMOTE} messageId="3" tabIndex={0} onImageClick={onImageClick} />);
    fireEvent.click(await screen.findByRole("button", { name: "View image cat.gif" }));
    expect(onImageClick).toHaveBeenCalledWith(GIF, expect.any(HTMLElement));
    expect(opener.openUrl).not.toHaveBeenCalled();
  });

  it("hides the image with the x button and tells the engine", async () => {
    vi.spyOn(api, "inlineImage").mockResolvedValue(GIF);
    const dismiss = vi.spyOn(api, "previewDismiss");
    renderMessage(`<img src="${REMOTE}">`, true);
    await screen.findByRole("figure");
    fireEvent.click(screen.getByRole("button", { name: "Hide preview" }));
    expect(screen.queryByRole("figure")).toBeNull();
    expect(dismiss).toHaveBeenCalledWith("1", REMOTE);
  });
});

describe("image viewer in the chat", () => {
  const OWN = "data:image/png;base64,iVBORw0KGgo=";
  const body = `look <img src="${OWN}" alt="x"> and <img src="${REMOTE}">`;

  beforeEach(() => {
    vi.spyOn(api, "inlineImage").mockResolvedValue(GIF);
  });

  it("opens on a message image, steps to the external image and returns focus on Escape", async () => {
    renderMessage(body, true);
    await screen.findByRole("figure", { name: "Image: cat.gif" });
    const sent = screen.getByRole("button", { name: /open in viewer/i });
    fireEvent.click(sent);
    const viewer = screen.getByRole("dialog", { name: "Image viewer" });
    expect(screen.getByText("1 / 2")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open in browser" })).toBeNull();
    fireEvent.keyDown(viewer, { key: "ArrowRight" });
    expect(screen.getByText("2 / 2")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open in browser" })).toBeInTheDocument();
    fireEvent.keyDown(viewer, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() => {
      expect(sent).toHaveFocus();
    });
  });

  it("opens from the inline external image and returns focus to its button", async () => {
    renderMessage(body, true);
    const view = await screen.findByRole("button", { name: /View image cat\.gif/ });
    fireEvent.click(view);
    expect(screen.getByText("2 / 2")).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    await waitFor(() => {
      expect(view).toHaveFocus();
    });
  });
});
