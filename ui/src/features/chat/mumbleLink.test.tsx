import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createMockApi,
  setApi,
  type ChatMessage,
  type MockApi,
  type MumbleLinkStats,
} from "../../api";
import { applyLanguage, i18n } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { clearDismissedPreviews } from "./dismissedPreviews";
import { clearPreviews } from "./links";
import {
  canonicalLink,
  credentialLinks,
  joinText,
  linkHasCredentials,
  mumbleLinks,
  parseMumbleLink,
  savedServer,
} from "./mumbleLink";
import { sanitizeHtml } from "./sanitize";
import { MessageList } from "./MessageList";

const LINK = "mumble://voice.example.org:64739/Raid/Officers";
const CREDENTIALS = "mumble://Maya:secret@voice.example.org:64739/Raid";
const ANSWER: MumbleLinkStats = {
  kind: "answered",
  pingMs: 4,
  users: 11,
  maxUsers: 20,
  version: "1.5.735",
};
const SERVER = { id: 3, label: "Office", address: "voice.example.org", port: 64739 };

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

function renderMessage(body: string, linkPreviews = true, dismissed: string[] = []) {
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
  clearPreviews();
  clearDismissedPreviews();
  api = createMockApi();
  setApi(api);
  vi.spyOn(api, "mumbleLinkStats").mockResolvedValue(ANSWER);
});

afterEach(() => {
  vi.restoreAllMocks();
  cleanup();
});

describe("reading a link", () => {
  it("takes the server, the port and the channel path", () => {
    expect(parseMumbleLink(LINK)).toEqual({
      address: "voice.example.org",
      port: 64739,
      username: null,
      password: null,
      channelPath: ["Raid", "Officers"],
    });
  });

  it("uses the standard port when the link names none", () => {
    expect(parseMumbleLink("mumble://voice.example.org/Lobby")?.port).toBe(64738);
  });

  it("decodes the names it reads", () => {
    const link = parseMumbleLink("mumble://host/R%69%65%73/M%C3%BCller");
    expect(link?.channelPath).toEqual(["Ries", "Müller"]);
  });

  it("refuses a link without a server or with a port out of range", () => {
    expect(parseMumbleLink("mumble:///Channel")).toBeNull();
    expect(parseMumbleLink("mumble://host:70000")).toBeNull();
    expect(parseMumbleLink("https://example.org/channel")).toBeNull();
  });

  it("keeps credentials out of the link the card shows and copies", () => {
    expect(linkHasCredentials(CREDENTIALS)).toBe(true);
    expect(canonicalLink(CREDENTIALS)).toBe("mumble://voice.example.org:64739/Raid");
  });

  it("names the card after a saved server, and says what joining would do", () => {
    const link = parseMumbleLink(LINK);
    if (link === null) throw new Error("the link should parse");
    expect(savedServer([SERVER], link)).toEqual(SERVER);
    expect(savedServer([], link)).toBeNull();
    const t = i18n.getFixedT("en", "chat");
    expect(joinText(t, SERVER, false)).toBe("Join");
    expect(joinText(t, SERVER, true)).toBe("Go to channel");
    expect(joinText(t, null, false)).toBe("Add to servers");
  });

  it("counts the links that carry credentials", () => {
    expect(credentialLinks([LINK, CREDENTIALS, "mumble://host/a"])).toBe(1);
  });
});

describe("finding the links of a message", () => {
  it("takes anchors and text, in order, without duplicates", () => {
    const html = `<a href="${LINK}">join</a> and ${CREDENTIALS} and ${LINK}`;
    expect(mumbleLinks(html)).toEqual([LINK, CREDENTIALS]);
  });

  it("shows at most three cards", () => {
    const html = [1, 2, 3, 4].map((n) => `mumble://host${String(n)}/`).join(" ");
    expect(mumbleLinks(html)).toHaveLength(3);
  });

  it("puts no mumble target into the page and never shows its credentials", () => {
    const url = "mumble://Maya:secret@voice.example.org:64739/Raid";
    const clean = sanitizeHtml(`<a href="${url}">${url}</a>`);
    expect(clean).not.toContain("secret");
    expect(clean).not.toContain("href=");
    expect(clean).toContain("mumble://voice.example.org:64739/Raid");
  });

  it("leaves other links as they are", () => {
    const clean = sanitizeHtml('<a href="https://example.org/x">story</a>');
    expect(clean).toContain('href="https://example.org/x"');
  });
});

describe("the card", () => {
  it("shows the server, the channel and the figures", async () => {
    renderMessage(`<a href="${LINK}">join</a>`);
    const card = await screen.findByRole("article", { name: "Server card for voice.example.org" });
    expect(card).toHaveTextContent("Channel Raid / Officers");
    expect(card).toHaveTextContent("Ping 4 ms · 11 of 20 users · server 1.5.735");
  });

  it("shows the server alone when the link has no channel", async () => {
    renderMessage("mumble://voice.example.org");
    const card = await screen.findByRole("article", { name: "Server card for voice.example.org" });
    expect(card.querySelector(".mumble-card__channel")).toBeNull();
  });

  it("shows a hint instead of figures while the privacy switch is off", async () => {
    // The card itself is message content, so it is shown even when the setting is off.
    vi.spyOn(api, "mumbleLinkStats").mockResolvedValue({ kind: "privacyOff" });
    renderMessage(`<a href="${LINK}">join</a>`, false);
    const card = await screen.findByRole("article", { name: "Server card for voice.example.org" });
    expect(card).toHaveTextContent("Server figures are switched off in the settings");
    expect(card).not.toHaveTextContent("Ping");
  });

  it("says when the server did not answer, and asks again on the card's button", async () => {
    const stats = vi.spyOn(api, "mumbleLinkStats");
    stats.mockResolvedValue({ kind: "unreachable" });
    renderMessage(`<a href="${LINK}">join</a>`);
    const card = await screen.findByRole("article", { name: "Server card for voice.example.org" });
    expect(card).toHaveTextContent("Server not reachable");

    stats.mockResolvedValue(ANSWER);
    fireEvent.click(screen.getByRole("button", { name: "Check again voice.example.org" }));
    await waitFor(() => {
      expect(card).toHaveTextContent("Ping 4 ms");
    });
    expect(stats).toHaveBeenLastCalledWith(LINK, true);
  });

  it("warns about credentials with the number of links that carry them", async () => {
    const other = CREDENTIALS.replace("voice.example.org", "other.example.org");
    renderMessage(`${CREDENTIALS} ${other}`);
    const card = await screen.findByRole("article", { name: "Server card for voice.example.org" });
    expect(card).toHaveTextContent("2 links in this message show credentials");
    const second = await screen.findByRole("article", {
      name: "Server card for other.example.org",
    });
    expect(second).toHaveTextContent("2 links in this message show credentials");
  });

  it("joins only when the card's Join is pressed, and says what it did", async () => {
    const connect = vi.spyOn(api, "connect");
    const join = vi.spyOn(api, "mumbleLinkJoin").mockResolvedValue({
      kind: "connecting",
      serverId: 1,
    });
    renderMessage(`<a href="${LINK}">join</a>`);
    const card = await screen.findByRole("article", { name: "Server card for voice.example.org" });
    expect(connect).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Join voice.example.org" }));
    await waitFor(() => {
      expect(join).toHaveBeenCalledWith(LINK);
      expect(card).toHaveTextContent("Joined");
    });
  });

  it("copies the link without its credentials", async () => {
    const copy = vi.spyOn(api, "copyText");
    renderMessage(`<a href="${CREDENTIALS}">join</a>`);
    await screen.findByRole("article", { name: "Server card for voice.example.org" });
    fireEvent.click(screen.getByRole("button", { name: "Copy the link to voice.example.org" }));
    expect(copy).toHaveBeenCalledWith("mumble://voice.example.org:64739/Raid");
  });

  it("dismisses the card and remembers it for this message", async () => {
    renderMessage(`<a href="${LINK}">join</a>`);
    await screen.findByRole("article", { name: "Server card for voice.example.org" });
    fireEvent.click(screen.getByRole("button", { name: "Hide preview" }));
    await waitFor(() => {
      expect(screen.queryByRole("article")).toBeNull();
    });

    renderMessage(`<a href="${LINK}">join</a>`, true, [LINK]);
    await waitFor(() => {
      expect(screen.queryByRole("article")).toBeNull();
    });
  });

  it("is labelled and has no accessibility violation", async () => {
    const { container } = renderMessage(`<a href="${CREDENTIALS}">join</a>`);
    await screen.findByRole("article", { name: "Server card for voice.example.org" });
    expect(await axeViolations(container)).toEqual([]);
  });
});
