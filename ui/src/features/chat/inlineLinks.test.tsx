import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type ChatMessage, type InlineImage, type MockApi } from "../../api";
import { applyLanguage } from "../../i18n";
import { clearDismissedPreviews } from "./dismissedPreviews";
import { clearInlineShown, hideShownLinks } from "./inlineLinks";
import { clearPreviews } from "./links";
import { MessageList } from "./MessageList";

const URL_A = "https://pics.example/cat.gif";
const link = (url: string) => `<a href="${url}">${url}</a>`;
const only = new Set([URL_A]);

describe("hideShownLinks", () => {
  it("hides a message that is only the link, with its line breaks", () => {
    expect(hideShownLinks(`${link(URL_A)}<br>`, only)).toBe("");
  });

  it("keeps the text of messages with other words", () => {
    const html = `look ${link(URL_A)} here`;
    expect(hideShownLinks(html, only)).toBe(html);
  });

  it("hides a link alone on the first or last line only", () => {
    expect(hideShownLinks(`${link(URL_A)}<br>nice one`, only)).toBe("nice one");
    expect(hideShownLinks(`nice one<br>${link(URL_A)}`, only)).toBe("nice one");
    const middle = `a<br>${link(URL_A)}<br>b`;
    expect(hideShownLinks(middle, only)).toBe(middle);
  });

  it("keeps links with their own text, other links and links without a shown image", () => {
    const named = `<a href="${URL_A}">my cat</a>`;
    expect(hideShownLinks(named, only)).toBe(named);
    const other = link("https://pics.example/dog.gif");
    expect(hideShownLinks(other, only)).toBe(other);
    expect(hideShownLinks(link(URL_A), new Set())).toBe(link(URL_A));
  });
});

const GIF: InlineImage = {
  url: URL_A,
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

function renderMessage(body: string) {
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
        linkPreviews
      />
    </main>,
  );
}

beforeAll(async () => {
  await applyLanguage("en");
});

let api: MockApi;

beforeEach(() => {
  clearPreviews();
  clearDismissedPreviews();
  clearInlineShown();
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
});

describe("link text under inline images", () => {
  it("replaces a lone link by its image and brings it back when the image is dismissed", async () => {
    vi.spyOn(api, "inlineImage").mockResolvedValue(GIF);
    const { container } = renderMessage(link(URL_A));
    const figure = await screen.findByRole("figure", { name: "Image: cat.gif" });
    await waitFor(() => {
      expect(container.querySelector(".chat-message__body a")).toBeNull();
    });
    expect(figure.querySelector("button[title]")?.getAttribute("title")).toBe(URL_A);
    fireEvent.click(screen.getByRole("button", { name: "Hide preview" }));
    await waitFor(() => {
      expect(container.querySelector(`.chat-message__body a[href="${URL_A}"]`)).not.toBeNull();
    });
  });

  it("shows a note with Open in browser when the image cannot be shown", async () => {
    vi.spyOn(api, "inlineImage").mockResolvedValue(null);
    const { container } = renderMessage(link(URL_A));
    expect(
      await screen.findByText("Image can't be shown (too large or unavailable)."),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Open/ })).toBeInTheDocument();
    expect(container.querySelector(`.chat-message__body a[href="${URL_A}"]`)).not.toBeNull();
  });
});
