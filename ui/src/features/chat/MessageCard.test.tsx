import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type ChatMessage, type InlineImage, type MockApi } from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { cardToHtml, type MessageCard } from "./cards";
import { clearPreviews } from "./links";
import { MessageList } from "./MessageList";

const IMAGE = "https://grafana.test/render/cpu.png";
const URL_ = "https://grafana.test/d/db-1";
const PNG: InlineImage = {
  url: IMAGE,
  src: "data:image/png;base64,iVBORw0KGgo=",
  width: 600,
  height: 200,
  animated: false,
};

function card(partial: Partial<MessageCard>): MessageCard {
  return { title: null, color: null, icon: null, image: null, url: null, body: "", ...partial };
}

const FULL = card({
  title: "CPU high on db-1",
  color: "warning",
  icon: "chart",
  image: IMAGE,
  url: URL_,
  body: "Load <b>97 %</b> with <code>postgres</code>",
});

function message(body: string): ChatMessage {
  return {
    id: "1",
    conversation: { kind: "channel", channelId: 1 },
    senderName: "Monitoring bot",
    senderCertHash: null,
    senderSession: null,
    own: false,
    sentAt: new Date().toISOString(),
    body,
    dismissedPreviews: [],
    syncedFrom: null,
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
          syncing: false,
          syncResult: null,
          syncFailed: false,
        }}
        label="Messages in #ops"
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
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
});

describe("message cards", () => {
  it("shows a full card as a labelled region with status text, body, image and link", async () => {
    const inlineImage = vi.spyOn(api, "inlineImage").mockResolvedValue(PNG);
    const { container } = renderMessage(cardToHtml(FULL), true);
    const region = screen.getByRole("region", { name: "Card: CPU high on db-1, Warning" });
    expect(region.textContent).toContain("Warning");
    expect(region.querySelector(".message-card__icon svg")).not.toBeNull();
    expect(region.querySelector(".message-card__body b")?.textContent).toBe("97 %");
    // The fallback lines for official clients do not show in Mumcord.
    expect(container.textContent).not.toContain("[Warning]");
    expect(container.textContent).not.toContain("Image:");
    const view = await screen.findByRole("button", { name: "View image CPU high on db-1" });
    expect(view.querySelector("img")?.getAttribute("src")).toBe(PNG.src);
    expect(inlineImage).toHaveBeenCalledWith(IMAGE);
    expect(screen.getByRole("link", { name: "CPU high on db-1" }).getAttribute("href")).toBe(URL_);
    expect(screen.getByRole("link", { name: "grafana.test/d/db-1" })).toBeTruthy();
    expect(await axeViolations(container)).toEqual([]);
  });

  it.each([
    ["info", "Info"],
    ["success", "Success"],
    ["warning", "Warning"],
    ["error", "Error"],
  ] as const)("names the %s status in text, not only by colour", (color, label) => {
    renderMessage(cardToHtml(card({ title: "Deploy", color, body: "x" })), false);
    const region = screen.getByRole("region", { name: `Card: Deploy, ${label}` });
    expect(region.dataset["color"]).toBe(color);
    expect(region.querySelector(".message-card__status")?.textContent).toContain(label);
  });

  it("shows a minimal card with title and link only", () => {
    renderMessage(cardToHtml(card({ title: "Incident 42", url: URL_ })), false);
    const region = screen.getByRole("region", { name: "Card: Incident 42" });
    expect(region.querySelector(".message-card__status")).toBeNull();
    expect(region.querySelector(".message-card__body")).toBeNull();
  });

  it("requests nothing with previews off and shows the image as a link", () => {
    const inlineImage = vi.spyOn(api, "inlineImage");
    renderMessage(cardToHtml(FULL), false);
    expect(inlineImage).not.toHaveBeenCalled();
    expect(document.querySelector("img")).toBeNull();
    const link = screen.getByRole("link", { name: "Image: grafana.test/render/cpu.png" });
    expect(link.getAttribute("href")).toBe(IMAGE);
  });

  it("falls back to the image link when the engine cannot load it", async () => {
    vi.spyOn(api, "inlineImage").mockResolvedValue(null);
    renderMessage(cardToHtml(FULL), true);
    expect(
      await screen.findByRole("link", { name: "Image: grafana.test/render/cpu.png" }),
    ).toBeTruthy();
  });

  it("opens the link with the Open button and offers the link menu on the link line", () => {
    const openLink = vi.spyOn(api, "openLink").mockResolvedValue(null);
    renderMessage(cardToHtml(card({ title: "Incident 42", url: URL_ })), false);
    fireEvent.click(screen.getByRole("button", { name: "Open Incident 42 in browser" }));
    expect(openLink).toHaveBeenCalledWith(URL_);
    fireEvent.contextMenu(screen.getByRole("link", { name: "grafana.test/d/db-1" }), {
      clientX: 10,
      clientY: 10,
    });
    expect(screen.getByRole("menuitem", { name: "Copy link" })).toBeTruthy();
  });

  it("does not add a link preview for the card's own link", () => {
    const linkPreview = vi.spyOn(api, "linkPreview");
    renderMessage(cardToHtml(card({ title: "Incident 42", url: URL_ })), true);
    expect(linkPreview).not.toHaveBeenCalled();
  });

  it("keeps the text around a card and renders typed BB code as a card", () => {
    renderMessage('On it:<br>[card title="Typed" color=error]Failover <b>active</b>[/card]', false);
    expect(screen.getByText("On it:")).toBeTruthy();
    const region = screen.getByRole("region", { name: "Card: Typed, Error" });
    expect(region.querySelector(".message-card__body")?.innerHTML).toBe("Failover <b>active</b>");
  });

  it("renders a hostile card as inert text", () => {
    renderMessage(
      '<div data-mumcord-card="title=%3Cimg%20src%3Dx%20onerror%3Dalert(1)%3E&amp;url=javascript%3Aalert(1)"><div data-mumcord-card="body"><a href="javascript:alert(2)">x</a></div></div>',
      false,
    );
    const region = screen.getByRole("region", { name: "Card: <img src=x onerror=alert(1)>" });
    expect(region.querySelector("img")).toBeNull();
    expect(region.querySelector("a[href^='javascript']")).toBeNull();
    expect(region.querySelector(".message-card__open")).toBeNull();
  });
});
