import { act, render } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import type { ChatMessage, Conversation } from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { MessageList } from "./MessageList";
import { addPrivateLine, resetPrivateLines, usePrivateLines } from "./privateLines";

/**
 * The line a finished admin command leaves in the chat (Epic 4, issue #378, FR-10.35,
 * mockup `05-result-line`): at its time among the messages, with the note that says who can see
 * it, and with none of a real message's actions.
 */
const CHANNEL: Conversation = { kind: "channel", channelId: 1 };

function message(id: string, body: string, sentAt: string): ChatMessage {
  return {
    id,
    conversation: CHANNEL,
    senderName: "Maya",
    senderCertHash: null,
    senderSession: 7,
    own: false,
    sentAt,
    body,
    dismissedPreviews: [],
    syncedFrom: null,
    file: null,
  };
}

function Chat() {
  // The chat shows the lines through the store, so a reset reaches the list (FR-10.63).
  const lines = usePrivateLines(1, CHANNEL);
  return (
    <main>
      <MessageList
        log={{
          messages: [
            message("1", "Hello there", "2026-11-03T10:00:00Z"),
            message("2", "See you later", "2026-11-03T10:05:00Z"),
          ],
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
        privateLines={lines}
      />
    </main>
  );
}

function add(text: string): void {
  addPrivateLine({ serverId: 1, conversation: CHANNEL, text, note: "Only you see this line" });
}

beforeAll(async () => {
  await applyLanguage("en");
});

afterEach(() => {
  resetPrivateLines();
});

describe("a line only its admin sees (FR-10.61)", () => {
  it("is drawn as a note, with the line saying who can see it", () => {
    add("You muted Anna on the server.");
    const { container } = render(<Chat />);
    const line = container.querySelector(".chat-message--private");
    expect(line?.textContent).toContain("You muted Anna on the server.");
    expect(line?.textContent).toContain("Only you see this line");
    // It is a note and not a message: no sender picture, and nothing to answer to (mockup 05).
    expect(container.querySelector(".chat-message--private .chat-avatar")).toBeNull();
  });

  it("lands after the older messages, which keep their order", () => {
    add("You moved Ben to AFK.");
    const { container } = render(<Chat />);
    const text = container.querySelector("ol")?.textContent ?? "";
    expect(text.indexOf("Hello there")).toBeLessThan(text.indexOf("See you later"));
    expect(text.indexOf("See you later")).toBeLessThan(text.indexOf("You moved Ben to AFK."));
    expect(container.querySelectorAll(".chat-message--private")).toHaveLength(1);
  });

  it("disappears with the line, because none of it is stored (FR-10.63)", () => {
    add("You muted Anna on the server.");
    const { container } = render(<Chat />);
    expect(container.querySelectorAll(".chat-message--private")).toHaveLength(1);
    act(() => {
      resetPrivateLines();
    });
    expect(container.querySelectorAll(".chat-message--private")).toHaveLength(0);
  });

  it("is clean for a screen reader", async () => {
    add("You muted Anna on the server.");
    const { container } = render(<Chat />);
    expect(await axeViolations(container)).toEqual([]);
  });
});
