import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createMockApi,
  EngineError,
  setApi,
  type ChatMessage,
  type InlineImage,
  type MockApi,
} from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { clearDismissedPreviews } from "./dismissedPreviews";
import { clearFavorites } from "./favorites";
import { FULL_ATTRIBUTE, resetFullImages } from "./fullImage";
import { imageFileName } from "./imageActions";
import { clearPreviews } from "./links";
import { MessageList } from "./MessageList";

const dialogs = vi.hoisted(() => ({ pickSavePath: vi.fn() }));
vi.mock("../settings/fileDialogs", () => dialogs);

const PNG = "data:image/png;base64,iVBORw0KGgo=";
const FULL_PNG = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUg==";
const CAT = "https://pics.example/cat.gif";
const GIF: InlineImage = {
  url: CAT,
  src: "data:image/gif;base64,R0lGODlhAQABAAAAACw=",
  width: 800,
  height: 400,
  animated: true,
};
const REFERENCE = `${"a".repeat(64)}:1234`;

let api: MockApi;

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
    syncedFrom: null,
  };
}

function renderMessage(body: string, linkPreviews = true) {
  return render(
    <main>
      <MessageList
        log={{
          messages: [message(body)],
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
        onReply={() => undefined}
        linkPreviews={linkPreviews}
      />
    </main>,
  );
}

function menuItems(): (string | null)[] {
  return within(screen.getByRole("menu"))
    .getAllByRole("menuitem")
    .map((item) => item.textContent);
}

function rightClick(element: Element) {
  fireEvent.contextMenu(element, { clientX: 20, clientY: 20 });
}

beforeAll(async () => {
  await applyLanguage("en");
});

beforeEach(() => {
  clearPreviews();
  clearDismissedPreviews();
  clearFavorites();
  resetFullImages();
  dialogs.pickSavePath.mockReset();
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
  window.getSelection()?.removeAllRanges();
});

describe("image context menu (#115)", () => {
  it("offers copy, save and the viewer for an image sent in a message", async () => {
    renderMessage(`<img src="${PNG}">`);
    const image = screen.getByRole("button", { name: /image from Maya/i });
    rightClick(image);
    const menu = screen.getByRole("menu", { name: "Image image" });
    expect(await axeViolations(menu)).toEqual([]);
    expect(menuItems()).toEqual(["Copy image", "Save image…", "Open in image viewer"]);
    fireEvent.click(screen.getByRole("menuitem", { name: "Open in image viewer" }));
    expect(screen.getByRole("dialog", { name: "Image viewer" })).toBeInTheDocument();
  });

  it("copies the shown image through the engine and confirms it", async () => {
    const copy = vi.spyOn(api, "copyImage");
    renderMessage(`<img src="${PNG}">`);
    rightClick(screen.getByRole("button", { name: /image from Maya/i }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy image" }));
    expect(await screen.findByText("Image copied")).toBeInTheDocument();
    expect(copy).toHaveBeenCalledWith(PNG);
  });

  it("copies the full-size image once it has arrived instead of the thumbnail", async () => {
    vi.spyOn(api, "fullImageFetch").mockResolvedValue(FULL_PNG);
    const copy = vi.spyOn(api, "copyImage");
    renderMessage(`<img src="${PNG}" ${FULL_ATTRIBUTE}="${REFERENCE}">`);
    // Opening the viewer fetches the full image.
    fireEvent.click(screen.getByRole("button", { name: /image from Maya/i }));
    await waitFor(() => {
      expect(document.querySelector(".image-viewer__img")?.getAttribute("src")).toBe(FULL_PNG);
    });
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    rightClick(screen.getByRole("button", { name: /image from Maya/i }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy image" }));
    await waitFor(() => {
      expect(copy).toHaveBeenCalledWith(FULL_PNG);
    });
  });

  it("saves the original bytes under a dated name in the real format", async () => {
    dialogs.pickSavePath.mockResolvedValue("/tmp/x.png");
    const save = vi.spyOn(api, "saveImage");
    renderMessage(`<img src="${PNG}">`);
    rightClick(screen.getByRole("button", { name: /image from Maya/i }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Save image…" }));
    expect(await screen.findByText("Image saved")).toBeInTheDocument();
    expect(dialogs.pickSavePath).toHaveBeenCalledWith(
      expect.stringMatching(/^image-\d{4}-\d{2}-\d{2}\.png$/),
      expect.objectContaining({ extensions: ["png"] }),
      "Save image",
    );
    expect(save).toHaveBeenCalledWith("/tmp/x.png", PNG);
  });

  it("shows an error when copying fails, without the image in the log", async () => {
    vi.spyOn(api, "copyImage").mockRejectedValue(new EngineError("io", "no clipboard"));
    const error = vi.spyOn(console, "error").mockImplementation(() => undefined);
    renderMessage(`<img src="${PNG}">`);
    rightClick(screen.getByRole("button", { name: /image from Maya/i }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy image" }));
    expect(await screen.findByText("The image could not be copied")).toBeInTheDocument();
    expect(JSON.stringify(error.mock.calls)).not.toContain("iVBOR");
  });

  it("says when an image is too large", async () => {
    vi.spyOn(api, "copyImage").mockRejectedValue(new EngineError("messageTooLarge", "too big"));
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    renderMessage(`<img src="${PNG}">`);
    rightClick(screen.getByRole("button", { name: /image from Maya/i }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy image" }));
    expect(await screen.findByText("The image is too large to copy or save")).toBeInTheDocument();
  });

  it("puts Copy for selected text first", () => {
    renderMessage(`hello <img src="${PNG}">`);
    const text = screen.getByText(/hello/).firstChild as Text;
    const range = document.createRange();
    range.setStart(text, 0);
    range.setEnd(text, 5);
    window.getSelection()?.addRange(range);
    rightClick(screen.getByRole("button", { name: /image from Maya/i }));
    expect(menuItems()[0]).toBe("Copy");
    expect(menuItems()[1]).toBe("Copy image");
  });

  it("offers everything for a loaded external GIF and saves the animated file", async () => {
    vi.spyOn(api, "inlineImage").mockResolvedValue(GIF);
    dialogs.pickSavePath.mockResolvedValue("/tmp/cat.gif");
    const save = vi.spyOn(api, "saveImage");
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    renderMessage(`<img src="${CAT}">`);
    const figure = await screen.findByRole("figure", { name: "Image: cat.gif" });
    rightClick(figure);
    expect(menuItems()).toEqual([
      "Copy image",
      "Save image…",
      "Copy image address",
      "Open in image viewer",
      "Save as favorite",
      "Open in browser",
    ]);
    fireEvent.click(screen.getByRole("menuitem", { name: "Save image…" }));
    await waitFor(() => {
      expect(save).toHaveBeenCalledWith("/tmp/cat.gif", GIF.src);
    });
    expect(dialogs.pickSavePath).toHaveBeenCalledWith(
      "cat.gif",
      expect.objectContaining({ extensions: ["gif"] }),
      "Save image",
    );
    rightClick(figure);
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy image address" }));
    expect(writeText).toHaveBeenCalledWith(CAT);
    expect(await screen.findByText("Image address copied")).toBeInTheDocument();
  });

  it("opens from the menu key on the focused image, placed at the image", () => {
    renderMessage(`<img src="${PNG}">`);
    const image = screen.getByRole("button", { name: /image from Maya/i });
    act(() => {
      image.focus();
    });
    fireEvent.contextMenu(image, { clientX: 0, clientY: 0 });
    expect(screen.getByRole("menuitem", { name: "Copy image" })).toHaveFocus();
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    expect(image).toHaveFocus();
  });

  it("offers only the address of an external image that failed to load, fetching nothing", async () => {
    const inline = vi.spyOn(api, "inlineImage").mockResolvedValue(null);
    const copy = vi.spyOn(api, "copyImage");
    renderMessage(`<img src="${CAT}">`);
    const failed = await screen.findByRole("group", { name: "Image: cat.gif" });
    expect(inline).toHaveBeenCalledTimes(1);
    rightClick(failed);
    expect(menuItems()).toEqual(["Copy image address"]);
    expect(inline).toHaveBeenCalledTimes(1);
    expect(copy).not.toHaveBeenCalled();
  });

  it("loads nothing for an external image when link previews are off", () => {
    const inline = vi.spyOn(api, "inlineImage");
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    renderMessage(`<a href="${CAT}">${CAT}</a>`, false);
    rightClick(screen.getByRole("link"));
    expect(menuItems()).toEqual(["Copy link", "Open link"]);
    expect(inline).not.toHaveBeenCalled();
    expect(fetchSpy).not.toHaveBeenCalled();
  });
});

describe("imageFileName", () => {
  const day = new Date(2026, 9, 29);
  it("keeps the own name with the real format's extension", () => {
    expect(imageFileName("cat.gif", "gif", day)).toBe("cat.gif");
    expect(imageFileName("cat.gif", "webp", day)).toBe("cat.webp");
    expect(imageFileName('a/b:"c"', "png", day)).toBe("a_b__c_.png");
  });
  it("dates images without a name", () => {
    expect(imageFileName(null, "jpg", day)).toBe("image-2026-10-29.jpg");
    expect(imageFileName("  ", "png", day)).toBe("image-2026-10-29.png");
  });
});
