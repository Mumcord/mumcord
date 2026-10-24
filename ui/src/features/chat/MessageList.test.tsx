import { render, screen, within } from "@testing-library/react";
import { beforeAll, describe, expect, it } from "vitest";
import type { ChatDisplay, ChatMessage } from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { MessageList } from "./MessageList";

const AVATAR = "data:image/png;base64,iVBORw0KGgo=";

function message(id: string, sender: string, minutesAgo: number, body: string): ChatMessage {
  return {
    id,
    conversation: { kind: "channel", channelId: 1 },
    senderName: sender,
    senderCertHash: `${sender}-hash`,
    senderSession: null,
    own: false,
    sentAt: new Date(Date.now() - minutesAgo * 60_000).toISOString(),
    body,
  };
}

const MESSAGES = [
  message("1", "Maya", 30, "first"),
  message("2", "Maya", 29, "second"),
  message("3", "Jonas", 28, "third"),
];

function renderList(display: ChatDisplay, messages = MESSAGES) {
  return render(
    <main>
      <MessageList
        log={{ messages, loaded: true, loading: false, hasMore: false, loadFailed: false }}
        label="Messages in #Lobby"
        onLoadOlder={() => undefined}
        display={display}
        avatarFor={(m) => (m.senderName === "Maya" ? AVATAR : null)}
      />
    </main>,
  );
}

beforeAll(async () => {
  await applyLanguage("en");
});

describe("MessageList cozy", () => {
  it("shows the avatar and name once per group, initials when there is no image", () => {
    const { container } = renderList("cozy");
    const items = screen.getAllByRole("listitem");
    expect(items.map((item) => item.querySelectorAll(".avatar").length)).toEqual([1, 0, 1]);
    expect(items[0]?.querySelector("img")).toHaveAttribute("src", AVATAR);
    expect(items[2]?.querySelector(".avatar")).toHaveAttribute("data-initial", "J");
    expect(container.querySelectorAll(".chat-message__header")).toHaveLength(2);
  });

  it("gives every message sender, time and text once, grouped ones included", () => {
    renderList("cozy");
    const items = screen.getAllByRole("listitem");
    const grouped = items[1] as HTMLElement;
    expect(within(grouped).getByText("Maya")).toHaveClass("visually-hidden");
    expect(grouped.querySelectorAll("time")).toHaveLength(1);
    expect(grouped).toHaveTextContent(/^Maya\s*\d\d:\d\d(\s[AP]M)?\s*second$/);
    expect(items[0]).toHaveTextContent(/Maya\s*\d\d:\d\d(\s[AP]M)?\s*first$/);
  });

  it("puts a day separator above the first message of each day", () => {
    const old = { ...message("0", "Priya", 0, "old"), sentAt: "2020-01-02T12:00:00Z" };
    renderList("cozy", [old, ...MESSAGES]);
    const days = document.querySelectorAll(".chat-day");
    expect([...days].map((d) => d.textContent)).toEqual([expect.stringContaining("2020"), "Today"]);
  });

  it("has no axe violations", async () => {
    renderList("cozy");
    expect(await axeViolations(document.body)).toEqual([]);
  });
});

describe("MessageList compact", () => {
  it("shows time, name and text on one line without avatars", async () => {
    renderList("compact");
    const list = screen.getByRole("list", { name: "Messages in #Lobby" });
    expect(list).toHaveAttribute("data-display", "compact");
    expect(list.querySelectorAll(".avatar")).toHaveLength(0);
    const items = screen.getAllByRole("listitem");
    expect(items[1]).toHaveTextContent(/^\d\d:\d\d(\s[AP]M)?\s*Maya\s*second$/);
    expect(within(items[1] as HTMLElement).getByText("Maya")).not.toHaveClass("visually-hidden");
    expect(await axeViolations(document.body)).toEqual([]);
  });
});
