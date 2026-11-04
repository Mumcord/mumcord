import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatMessage, Conversation } from "../../api";
import { connectedMock, resetAll } from "../channels/testUtils";
import { ensureLoaded, resetChatStore, useConversationLog } from "./chatStore";
import {
  historyImageReferences,
  IMAGE_REF_ATTRIBUTE,
  loadHistoryImage,
  PLACEHOLDER_CLASS,
  resetHistoryImages,
  useHistoryImageStates,
} from "./historyImages";
import { sanitizeHtml } from "./sanitize";

const LOBBY: Conversation = { kind: "channel", channelId: 1 };
const REFERENCE = `${"ab".repeat(32)}:1200:png:640x480`;
const WAITING = `<p>look</p><img ${IMAGE_REF_ATTRIBUTE}="${REFERENCE}">`;
const PNG = "data:image/png;base64,iVBORw0KGgo=";

function synced(id: string, body: string): ChatMessage {
  return {
    id,
    conversation: LOBBY,
    senderName: "Maya",
    senderCertHash: "maya-hash",
    senderSession: null,
    own: false,
    sentAt: new Date(Date.now() - 60_000).toISOString(),
    body,
    dismissedPreviews: [],
    syncedFrom: "Alice",
    file: null,
  };
}

afterEach(() => {
  vi.restoreAllMocks();
  resetChatStore();
  resetHistoryImages();
  resetAll();
});

describe("referenced images of synced messages (FR-1.60f)", () => {
  it("become placeholders of the image's size", () => {
    const html = sanitizeHtml(WAITING);
    const template = document.createElement("template");
    template.innerHTML = html;
    const placeholder = template.content.querySelector<HTMLElement>(`.${PLACEHOLDER_CLASS}`);
    expect(placeholder?.style.width).toBe("640px");
    expect(placeholder?.style.aspectRatio).toBe("640 / 480");
    expect(template.content.querySelector("img")).toBeNull();
    expect(historyImageReferences(html)).toEqual([REFERENCE]);
  });

  it("drop malformed references and keep real images untouched", () => {
    expect(sanitizeHtml(`<img ${IMAGE_REF_ATTRIBUTE}="${REFERENCE}:x">`)).toBe("");
    expect(sanitizeHtml(`<img ${IMAGE_REF_ATTRIBUTE}="${REFERENCE}" onerror="x()">`)).not.toContain(
      "onerror",
    );
    expect(sanitizeHtml(`<img src="${PNG}" ${IMAGE_REF_ATTRIBUTE}="${REFERENCE}">`)).toBe(
      `<img src="${PNG}">`,
    );
  });

  it("replace the placeholder when the engine sends the updated message", async () => {
    const api = await connectedMock({ bindStore: true });
    resetChatStore();
    const { result } = renderHook(() => useConversationLog(LOBBY));
    act(() => {
      ensureLoaded(LOBBY);
    });
    await waitFor(() => {
      expect(result.current.loaded).toBe(true);
    });
    act(() => {
      api.mock.emit("historySynced", {
        serverId: 1,
        channelId: 1,
        messages: [synced("s", WAITING)],
      });
    });
    const restored = `<p>look</p><img src="${PNG}">`;
    act(() => {
      api.mock.emit("historySynced", {
        serverId: 1,
        channelId: 1,
        messages: [synced("s", restored)],
      });
    });
    expect(result.current.messages.map((m) => m.body)).toEqual([restored]);
  });

  it("track the engine's hints and load by hand", async () => {
    const api = await connectedMock({ bindStore: true });
    const load = vi.spyOn(api, "historyImageLoad");
    // The chat store listens to the engine once a conversation is shown.
    renderHook(() => useConversationLog(LOBBY));
    act(() => {
      ensureLoaded(LOBBY);
    });
    const { result } = renderHook(() => useHistoryImageStates());
    act(() => {
      api.mock.emit("historyImageStatus", { serverId: 1, reference: REFERENCE, state: "manual" });
    });
    expect(result.current(1, REFERENCE)).toBe("manual");
    expect(result.current(2, REFERENCE)).toBeUndefined();
    act(() => {
      loadHistoryImage(1, REFERENCE);
    });
    expect(load).toHaveBeenCalledWith(1, REFERENCE);
    expect(result.current(1, REFERENCE)).toBeUndefined();
  });
});
