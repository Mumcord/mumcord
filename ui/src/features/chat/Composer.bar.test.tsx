// The message input bar of issue #596: one bar with the field above the icon row, icons of one
// size, a send button that says whether there is something to send, a field that grows with the
// text, and a locked note instead of an input without the text-message permission.
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi } from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { connectedMock, resetAll } from "../channels/testUtils";
import { Composer } from "./Composer";
import type { SendTarget } from "./conversationInfo";

const prepare = vi.hoisted(() => vi.fn());
vi.mock("./images", async (original) => ({
  ...(await original<typeof import("./images")>()),
  prepareImage: prepare,
}));

const LIMITS = { text: 5000, image: 100_000, allowHtml: true };
const CHANNEL = { kind: "channel", channelId: 1 } as const;
const BLOCKED = "You don't have permission to write in this channel.";

function png(name: string): File {
  return new File(["x"], name, { type: "image/png" });
}

function ready(tag: string) {
  return {
    ok: true,
    dataUrl: `data:image/png;base64,${tag}`,
    width: 800,
    height: 600,
    scaled: false,
  };
}

function setup(target: SendTarget = CHANNEL, blockedReason: string | null = null) {
  const { container } = render(
    <Composer
      target={target}
      limits={LIMITS}
      label="Message #Lobby"
      blockedReason={blockedReason}
    />,
  );
  const bar = container.querySelector<HTMLElement>(".chat-composer__bar");
  const input = screen.queryByRole<HTMLTextAreaElement>("textbox", { name: "Message #Lobby" });
  const fileInput = container.querySelector<HTMLInputElement>('input[type="file"]');
  return { container, bar, input, fileInput };
}

function sendButton() {
  return screen.getByRole("button", { name: "Send" });
}

let api: MockApi;

beforeAll(async () => {
  await applyLanguage("en");
  // jsdom lays nothing out: measure the field by its lines the way a browser's scrollHeight would.
  Object.defineProperty(HTMLTextAreaElement.prototype, "scrollHeight", {
    configurable: true,
    get(this: HTMLTextAreaElement) {
      return 24 * Math.max(1, this.value.split("\n").length);
    },
  });
});

beforeEach(async () => {
  prepare.mockReset();
  prepare.mockImplementation((file: File) => Promise.resolve(ready(file.name)));
  api = await connectedMock({ bindStore: true });
});

afterEach(() => {
  resetAll();
});

describe("Composer bar", () => {
  it("puts the field above the icon row in one bar, with the toolbar inside it", () => {
    const { bar, input } = setup();
    const tools = bar?.querySelector<HTMLElement>(".chat-composer__tools") ?? null;
    if (bar === null || input === null || tools === null) {
      throw new Error("the composer renders no bar with a field and an icon row");
    }
    // The field is a row of its own, so its placeholder starts at the bar's left edge.
    expect(bar.querySelector(".chat-composer__input")).toBe(input);
    expect(input.compareDocumentPosition(tools) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // The formatting pill belongs to the bar: that is what draws the single focus ring.
    expect(bar.querySelector(".chat-format")).not.toBeNull();
  });

  it("draws every icon of the row at the same size and weight", () => {
    const { container } = setup();
    // The send arrow is smaller by design (mockup); every icon of the row itself is 20 px.
    const icons = [
      ...container.querySelectorAll<SVGSVGElement>(".chat-composer__tools svg"),
    ].filter((icon) => icon.closest(".chat-composer__send") === null);
    expect(icons.length).toBeGreaterThanOrEqual(2);
    for (const icon of icons) {
      expect(icon.getAttribute("width")).toBe("20");
      expect(icon.getAttribute("height")).toBe("20");
      expect(icon.getAttribute("stroke-width")).toBe("1.75");
      expect(icon.getAttribute("stroke")).toBe("currentColor");
    }
  });

  it("keeps the same colour on the paperclip as on the other icons", () => {
    const { container } = setup();
    const clip = screen.getByRole("button", { name: "Share files in this channel" });
    const image = screen.getByRole("button", { name: "Add image" });
    expect(clip.className).toBe(image.className);
    // A paperclip that works is not marked disabled: only an unavailable share says so.
    expect(clip.getAttribute("aria-disabled")).toBeNull();
    expect(container.querySelector(".chat-composer__icon[aria-disabled='true']")).toBeNull();
  });

  it("mutes send until there is text", () => {
    const { input } = setup();
    expect(sendButton()).toHaveAttribute("aria-disabled", "true");
    fireEvent.change(input as HTMLTextAreaElement, { target: { value: "  " } });
    expect(sendButton()).toHaveAttribute("aria-disabled", "true");
    fireEvent.change(input as HTMLTextAreaElement, { target: { value: "hi" } });
    expect(sendButton().getAttribute("aria-disabled")).toBeNull();
  });

  it("sends an image on its own and asks only for an optional comment", async () => {
    const { input, fileInput } = setup();
    expect(input?.placeholder).toBe("Message #Lobby");
    fireEvent.change(fileInput as HTMLInputElement, { target: { files: [png("shot.png")] } });
    // Still being processed: nothing can go out yet.
    expect(sendButton()).toHaveAttribute("aria-disabled", "true");
    await screen.findByText(/800×600/);
    expect(input?.placeholder).toBe("Add a message (optional)");
    expect(sendButton().getAttribute("aria-disabled")).toBeNull();
    // The name still says what the button does; the label is only shown next to it.
    expect(sendButton()).toHaveTextContent("Send");
  });

  it("grows the field with the text and shrinks it again", () => {
    const { input } = setup();
    const one = input?.style.height;
    fireEvent.change(input as HTMLTextAreaElement, { target: { value: "one line" } });
    expect(input?.style.height).toBe(one);
    fireEvent.change(input as HTMLTextAreaElement, { target: { value: "a\nb\nc" } });
    expect(input?.style.height).toBe("72px");
    fireEvent.change(input as HTMLTextAreaElement, { target: { value: "a\nb" } });
    expect(input?.style.height).toBe("48px");
  });

  it("has nothing to send and no field at all without the write permission", () => {
    const { container, input } = setup(CHANNEL, BLOCKED);
    expect(input).toBeNull();
    expect(container.querySelector(".chat-composer__input")).toBeNull();
    expect(container.querySelector(".chat-format")).toBeNull();
    expect(screen.queryByRole("button", { name: "Send" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Add image" })).toBeNull();
    expect(screen.getByText(BLOCKED)).toBeInTheDocument();
    expect(container.querySelector(".chat-composer__bar--locked")).not.toBeNull();
  });

  it("keeps the Apps button alive when writing is blocked but the voice channel is here", async () => {
    api.mock.emit("voiceServerChanged", { serverId: 1 });
    const { selectConversation } = await import("../channels/selection");
    const { getServerState } = await import("../channels/serverStore");
    const { users, selfSession } = getServerState();
    act(() => {
      selectConversation({
        kind: "channel",
        channelId: users.get(selfSession ?? -1)?.channelId ?? -1,
      });
    });
    const { container } = setup(CHANNEL, BLOCKED);
    const apps = screen.getByRole("button", { name: "Apps" });
    expect(apps).toBeEnabled();
    expect(container.querySelector(".chat-composer__tools .chat-apps")).toBe(apps.parentElement);
  });

  it("passes axe with an attachment in the bar", async () => {
    const { container, fileInput } = setup();
    fireEvent.change(fileInput as HTMLInputElement, { target: { files: [png("shot.png")] } });
    await screen.findByRole("listitem");
    expect(await axeViolations(container)).toEqual([]);
  });
});
