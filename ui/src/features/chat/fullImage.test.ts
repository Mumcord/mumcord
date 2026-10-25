import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../api";
import { applyLanguage } from "../../i18n";
import {
  FULL_ATTRIBUTE_RESERVE,
  FULL_REFERENCE,
  fullImageSource,
  fullImageTag,
  keepFullImage,
  resetFullImages,
} from "./fullImage";
import { sanitizeHtml } from "./sanitize";
import { useComposer } from "./useComposer";

const images = vi.hoisted(() => ({ prepareImage: vi.fn() }));
vi.mock("./images", async (original) => ({
  ...(await original<typeof import("./images")>()),
  prepareImage: images.prepareImage,
}));

const THUMB = "data:image/jpeg;base64,/9j/AAAA";
const REFERENCE = `${"ab".repeat(32)}:524288`;
const LIMITS = { text: 5000, image: 100_000, allowHtml: true };
let api: MockApi;

beforeAll(async () => {
  await applyLanguage("en");
});

beforeEach(() => {
  api = createMockApi();
  setApi(api);
  resetFullImages();
  images.prepareImage.mockReset();
});

afterEach(() => {
  setApi(undefined);
});

describe("references", () => {
  it("accepts only the exact engine format", () => {
    expect(FULL_REFERENCE.test(REFERENCE)).toBe(true);
    for (const bad of [
      `${"AB".repeat(32)}:1`,
      `${"ab".repeat(32)}:0`,
      `${"ab".repeat(32)}:01`,
      `${"ab".repeat(31)}:1`,
      `${"ab".repeat(32)}:1"><script>`,
    ]) {
      expect(FULL_REFERENCE.test(bad), bad).toBe(false);
    }
  });

  it("reserves room for the longest attribute", () => {
    const tag = fullImageTag("", `${"ab".repeat(32)}:4294967295`);
    expect(tag.length - fullImageTag("", null).length).toBeLessThanOrEqual(FULL_ATTRIBUTE_RESERVE);
  });

  it("survives sanitizing only when valid", () => {
    const png = "data:image/png;base64,iVBORw0KGgo=";
    expect(sanitizeHtml(fullImageTag(png, REFERENCE))).toContain(
      `data-mumcord-full="${REFERENCE}"`,
    );
    expect(sanitizeHtml(`<img src="${png}" data-mumcord-full="javascript:x">`)).not.toContain(
      "data-mumcord-full",
    );
    expect(sanitizeHtml(`<img src="${png}" data-other="1">`)).not.toContain("data-other");
  });

  it("reads the source off a rendered image", () => {
    const img = document.createElement("img");
    expect(fullImageSource(img)).toBeUndefined();
    img.setAttribute("data-mumcord-full", REFERENCE);
    img.dataset["senderSession"] = "7";
    expect(fullImageSource(img)).toEqual({
      reference: REFERENCE,
      senderSession: 7,
      senderCertHash: null,
    });
    img.dataset["senderSession"] = "x";
    img.dataset["senderCert"] = "cert";
    expect(fullImageSource(img)).toEqual({
      reference: REFERENCE,
      senderSession: null,
      senderCertHash: "cert",
    });
  });
});

describe("keepFullImage", () => {
  const file = new Blob(["original picture"], { type: "image/png" });

  it("keeps the original when the thumbnail differs", async () => {
    const keep = vi.spyOn(api, "fullImageKeep");
    const reference = await keepFullImage(file, THUMB);
    expect(reference).toMatch(FULL_REFERENCE);
    expect(keep).toHaveBeenCalledWith(expect.stringMatching(/^data:image\/png;base64,/));
  });

  it("keeps nothing when the thumbnail is the original", async () => {
    const keep = vi.spyOn(api, "fullImageKeep");
    const original = "data:image/png;base64,b3JpZ2luYWwgcGljdHVyZQ==";
    expect(await keepFullImage(file, original)).toBeNull();
    expect(keep).not.toHaveBeenCalled();
  });

  it("falls back to the thumbnail when the engine refuses", async () => {
    vi.spyOn(api, "fullImageKeep").mockRejectedValue(new Error("invalid"));
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    expect(await keepFullImage(file, THUMB)).toBeNull();
  });
});

describe("sending", () => {
  it("puts the reference of a shrunk image into the message", async () => {
    images.prepareImage.mockResolvedValue({
      ok: true,
      dataUrl: THUMB,
      width: 1920,
      height: 1080,
      scaled: true,
    });
    vi.spyOn(api, "fullImageKeep").mockResolvedValue(REFERENCE);
    const send = vi.spyOn(api, "sendChannelMessage").mockImplementation((channelId, html) =>
      Promise.resolve({
        id: "m1",
        conversation: { kind: "channel", channelId },
        senderName: "me",
        senderCertHash: null,
        senderSession: 1,
        own: true,
        sentAt: "2026-10-25T10:00:00.000Z",
        body: html,
        dismissedPreviews: [],
        syncedFrom: null,
      }),
    );
    const { result } = renderHook(() => useComposer({ kind: "channel", channelId: 0 }, LIMITS));
    act(() => {
      result.current.addFiles([new File(["big"], "big.png", { type: "image/png" })]);
    });
    await waitFor(() => {
      expect(result.current.attachments[0]?.status).toBe("ready");
    });
    await act(async () => {
      await result.current.submit();
    });
    expect(send).toHaveBeenCalledWith(0, `<img src="${THUMB}" data-mumcord-full="${REFERENCE}" />`);
    const budget = (images.prepareImage.mock.calls[0] as [Blob, number])[1];
    expect(budget).toBeLessThanOrEqual(LIMITS.image - FULL_ATTRIBUTE_RESERVE);
  });
});
