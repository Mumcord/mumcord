import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type ChatMessage, type InlineImage, type MockApi } from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { Composer } from "./Composer";
import { clearDismissedPreviews } from "./dismissedPreviews";
import { clearFavorites } from "./favorites";
import { ImageViewer } from "./ImageViewer";
import { clearPreviews } from "./links";
import { MessageList } from "./MessageList";

const dialogs = vi.hoisted(() => ({ pickSavePath: vi.fn() }));
vi.mock("../settings/fileDialogs", () => dialogs);

const CAT = "https://pics.example/cat.gif";
const DOG = "https://pics.example/dog.gif";
const GIF: InlineImage = {
  url: CAT,
  src: "data:image/gif;base64,R0lGODlhAQABAAAAACw=",
  width: 800,
  height: 400,
  animated: true,
};
const LIMITS = { text: 5000, image: 100_000, allowHtml: true };

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

function renderMessage() {
  return render(
    <main>
      <MessageList
        log={{
          messages: [message(`<img src="${CAT}">`)],
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
        linkPreviews
      />
    </main>,
  );
}

function renderComposer(linkPreviews: boolean) {
  return render(
    <main>
      <Composer
        target={{ kind: "channel", channelId: 1 }}
        limits={LIMITS}
        label="Message #Lobby"
        blockedReason={null}
        linkPreviews={linkPreviews}
      />
    </main>,
  );
}

beforeAll(async () => {
  await applyLanguage("en");
});

beforeEach(() => {
  clearPreviews();
  clearDismissedPreviews();
  clearFavorites();
  api = createMockApi();
  setApi(api);
  vi.spyOn(api, "inlineImage").mockImplementation((url) => Promise.resolve({ ...GIF, url }));
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
});

describe("saving favorites", () => {
  it("saves and removes from the star in the image's hover actions", async () => {
    renderMessage();
    fireEvent.click(await screen.findByRole("button", { name: "Save as favorite" }));
    await waitFor(async () => {
      expect(await api.gifFavorites()).toEqual([CAT]);
    });
    fireEvent.click(screen.getByRole("button", { name: "Remove from favorites" }));
    await waitFor(async () => {
      expect(await api.gifFavorites()).toEqual([]);
    });
  });

  it("saves from the right-click menu on the image", async () => {
    renderMessage();
    const figure = await screen.findByRole("figure", { name: "Image: cat.gif" });
    fireEvent.contextMenu(figure, { clientX: 40, clientY: 40 });
    const menu = screen.getByRole("menu", { name: "Image cat.gif" });
    expect(await axeViolations(menu)).toEqual([]);
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Save as favorite" }));
    expect(screen.queryByRole("menu")).toBeNull();
    await waitFor(async () => {
      expect(await api.gifFavorites()).toEqual([CAT]);
    });
    fireEvent.contextMenu(figure, { clientX: 40, clientY: 40 });
    expect(screen.getByRole("menuitem", { name: "Remove from favorites" })).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("saves from the image viewer toolbar, only for external images", async () => {
    render(
      <ImageViewer
        images={[
          { src: GIF.src, name: "local" },
          { src: GIF.src, url: CAT, name: "cat.gif" },
        ]}
        index={1}
        onIndexChange={() => undefined}
        onClose={() => undefined}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Save as favorite" }));
    await waitFor(async () => {
      expect(await api.gifFavorites()).toEqual([CAT]);
    });
    expect(screen.getByRole("button", { name: "Remove from favorites" })).toBeInTheDocument();
  });
});

describe("GIF picker", () => {
  it("explains how to add favorites when there are none", async () => {
    renderComposer(true);
    fireEvent.click(screen.getByRole("button", { name: "GIF favorites" }));
    const dialog = await screen.findByRole("dialog", { name: "GIF favorites" });
    expect(dialog).toHaveTextContent("No favorites yet");
    expect(dialog).toHaveFocus();
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("button", { name: "GIF favorites" })).toHaveFocus();
  });

  async function openWithTwo(linkPreviews = true) {
    await api.gifFavoriteAdd(CAT);
    await api.gifFavoriteAdd(DOG);
    const view = renderComposer(linkPreviews);
    fireEvent.click(screen.getByRole("button", { name: "GIF favorites" }));
    const dialog = await screen.findByRole("dialog");
    const first = await within(dialog).findByRole("button", { name: "Send dog.gif" });
    return { view, dialog, first };
  }

  it("credits Giphy and Tenor favorites with a link to the original page", async () => {
    await api.gifFavoriteAdd("https://media.giphy.com/media/1ojn1S7BTXUry8elNi/giphy.gif");
    await api.gifFavoriteAdd(CAT);
    renderComposer(true);
    fireEvent.click(screen.getByRole("button", { name: "GIF favorites" }));
    const dialog = await screen.findByRole("dialog");
    const link = await within(dialog).findByRole("link", { name: "via GIPHY" });
    const open = vi.spyOn(window, "open").mockReturnValue(null);
    fireEvent.click(link);
    expect(open).toHaveBeenCalledWith(
      "https://giphy.com/gifs/1ojn1S7BTXUry8elNi",
      "_blank",
      "noopener,noreferrer",
    );
    expect(within(dialog).getAllByRole("link")).toHaveLength(1);
  });

  it("moves with the arrow keys and sends the chosen link as a message", async () => {
    const send = vi.spyOn(api, "sendChannelMessage").mockResolvedValue(message("sent"));
    const { dialog, first } = await openWithTwo();
    await waitFor(() => {
      expect(first).toHaveFocus();
    });
    fireEvent.keyDown(first, { key: "ArrowRight" });
    const second = within(dialog).getByRole("button", { name: "Send cat.gif" });
    expect(second).toHaveFocus();
    fireEvent.keyDown(second, { key: "ArrowLeft" });
    expect(first).toHaveFocus();
    fireEvent.keyDown(first, { key: "End" });
    expect(second).toHaveFocus();
    fireEvent.click(second);
    await waitFor(() => {
      expect(send).toHaveBeenCalledTimes(1);
    });
    expect(send.mock.calls[0]?.[1]).toContain(CAT);
    expect(screen.queryByRole("dialog")).toBeNull();
    // Sending moved it to the front.
    await waitFor(async () => {
      expect(await api.gifFavorites()).toEqual([CAT, DOG]);
    });
  });

  it("removes a favorite with the × button and with Delete", async () => {
    const { dialog, first } = await openWithTwo();
    fireEvent.click(within(dialog).getByRole("button", { name: "Remove cat.gif from favorites" }));
    await waitFor(() => {
      expect(within(dialog).queryByRole("button", { name: "Send cat.gif" })).toBeNull();
    });
    fireEvent.keyDown(first, { key: "Delete" });
    await waitFor(async () => {
      expect(await api.gifFavorites()).toEqual([]);
    });
    expect(dialog).toHaveTextContent("No favorites yet");
  });

  it("loads thumbnails through the engine only while link previews are on", async () => {
    const on = await openWithTwo(true);
    await waitFor(() => {
      expect(on.first.querySelector("img")).not.toBeNull();
    });
    expect(api.inlineImage).toHaveBeenCalledWith(DOG);
    on.view.unmount();
    clearPreviews();
    vi.mocked(api.inlineImage).mockClear();
    const off = await openWithTwo(false);
    expect(off.first.querySelector("img")).toBeNull();
    expect(off.first).toHaveTextContent("dog.gif");
    await act(() => Promise.resolve());
    expect(api.inlineImage).not.toHaveBeenCalled();
  });
});
