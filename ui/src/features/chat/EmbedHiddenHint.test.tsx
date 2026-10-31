import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type ChatMessage } from "../../api";
import { fakeView } from "../../app/testView";
import { ViewContext } from "../../app/view";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { clearDismissedPreviews } from "./dismissedPreviews";
import { EmbedHiddenHint } from "./EmbedHiddenHint";
import { hiddenEmbedKind } from "./embedHint";
import { clearPreviews } from "./links";
import { MessageList } from "./MessageList";

const PRIVACY = "Settings > Privacy";
const TAIL = `hidden because link previews and embeds are off in ${PRIVACY}.`;

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
  };
}

function renderMessage(
  body: string,
  linkPreviews: boolean,
  {
    dismissed = [],
    openPrivacy = vi.fn(),
  }: { dismissed?: string[]; openPrivacy?: () => void } = {},
) {
  return render(
    <ViewContext.Provider value={fakeView({ openPrivacySettings: openPrivacy })}>
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
      </main>
    </ViewContext.Provider>,
  );
}

const hint = (container: HTMLElement) => container.querySelector(".embed-hint");

beforeAll(async () => {
  await applyLanguage("en");
});

beforeEach(() => {
  clearPreviews();
  clearDismissedPreviews();
  setApi(createMockApi());
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  setApi(undefined);
});

describe("hiddenEmbedKind", () => {
  it("names images, GIFs, players and link cards from the address alone", () => {
    expect(hiddenEmbedKind("https://files.example/a/harbor.jpg", [])).toBe("image");
    expect(hiddenEmbedKind("https://files.example/pic", ["https://files.example/pic"])).toBe(
      "image",
    );
    expect(hiddenEmbedKind("https://files.example/dance.GIF", [])).toBe("gif");
    expect(hiddenEmbedKind("https://giphy.com/gifs/funny-cat-abcdEFGH1234", [])).toBe("gif");
    expect(hiddenEmbedKind("https://www.youtube.com/watch?v=dQw4w9WgXcQ", [])).toBe("video");
    expect(hiddenEmbedKind("https://youtu.be/dQw4w9WgXcQ", [])).toBe("video");
    expect(hiddenEmbedKind("https://www.twitch.tv/somechannel", [])).toBe("video");
    expect(hiddenEmbedKind("https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC", [])).toBe(
      "audio",
    );
    expect(hiddenEmbedKind("https://blog.example/posts/low-latency", [])).toBe("link");
  });
});

describe("EmbedHiddenHint", () => {
  it.each([
    ["image", "Image"],
    ["gif", "GIF"],
    ["video", "Video player"],
    ["audio", "Audio player"],
    ["link", "Link preview"],
  ] as const)("names a hidden %s", (kind, label) => {
    const { container } = render(<EmbedHiddenHint kinds={[kind]} />);
    expect(hint(container)?.textContent).toBe(`${label} ${TAIL}`);
  });

  it("renders nothing without hidden embeds", () => {
    const { container } = render(<EmbedHiddenHint kinds={[]} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("keeps the settings path as plain text when no settings page can be opened", () => {
    render(<EmbedHiddenHint kinds={["image"]} />);
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.getByText(PRIVACY)).toBeVisible();
  });
});

describe("hint under a message", () => {
  it("shows the hint when previews are off, opens Settings > Privacy and passes axe", async () => {
    const openPrivacy = vi.fn();
    const { container } = renderMessage(
      'look <a href="https://files.example/harbor.jpg">https://files.example/harbor.jpg</a>',
      false,
      { openPrivacy },
    );
    expect(hint(container)?.textContent).toBe(`Image ${TAIL}`);
    // Real text, not a live region: it is read with its message, not announced on every render.
    expect(container.querySelector(".embed-hint[role], .embed-hint [aria-live]")).toBeNull();
    const link = screen.getByRole("button", { name: PRIVACY });
    fireEvent.click(link);
    expect(openPrivacy).toHaveBeenCalledOnce();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("combines several hidden embeds into one hint", () => {
    const { container } = renderMessage(
      '<a href="https://files.example/harbor.jpg">a</a> ' +
        '<a href="https://www.youtube.com/watch?v=dQw4w9WgXcQ">b</a> ' +
        '<a href="https://blog.example/post">c</a>',
      false,
    );
    expect(container.querySelectorAll(".embed-hint")).toHaveLength(1);
    expect(hint(container)?.textContent).toBe(`3 previews ${TAIL}`);
  });

  it("counts an external image and a link once each", () => {
    const { container } = renderMessage(
      '<img src="https://files.example/pic.png"> <a href="https://blog.example/post">c</a>',
      false,
    );
    expect(hint(container)?.textContent).toBe(`2 previews ${TAIL}`);
  });

  it("shows no hint when previews are on", () => {
    const { container } = renderMessage('<a href="https://blog.example/post">c</a>', true);
    expect(hint(container)).toBeNull();
  });

  it("shows no hint for links that never embed", () => {
    const { container } = renderMessage(
      'mail <a href="mailto:a@b.example">me</a> or <a href="mumble://voice.example/">join</a>',
      false,
    );
    expect(hint(container)).toBeNull();
  });

  it("shows no hint for a card the user dismissed earlier", () => {
    const { container } = renderMessage('<a href="https://blog.example/post">c</a>', false, {
      dismissed: ["https://blog.example/post"],
    });
    expect(hint(container)).toBeNull();
  });
});
