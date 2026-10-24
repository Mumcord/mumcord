import { describe, expect, it } from "vitest";
import {
  fitImage,
  MAX_IMAGE_EDGE,
  messageLimits,
  UNLIMITED_IMAGE_FALLBACK,
  type Encoder,
} from "./images";

/** Fake encoder: output length grows with pixel count and quality. */
function fakeEncoder(calls: { width: number; height: number; quality: number }[]): Encoder {
  return (width, height, quality) => {
    calls.push({ width, height, quality });
    return Promise.resolve("x".repeat(Math.round(width * height * quality)));
  };
}

describe("fitImage", () => {
  it("starts at 600 px and lowers the quality first", async () => {
    const calls: { width: number; height: number; quality: number }[] = [];
    const url = await fitImage({ width: 1200, height: 600 }, fakeEncoder(calls), 600 * 300 * 0.6);
    expect(url).not.toBeNull();
    expect(calls[0]).toEqual({ width: MAX_IMAGE_EDGE, height: 300, quality: 0.85 });
    expect(calls.at(-1)).toEqual({ width: 600, height: 300, quality: 0.55 });
  });

  it("shrinks the image when the lowest quality is still too big", async () => {
    const calls: { width: number; height: number; quality: number }[] = [];
    const url = await fitImage({ width: 600, height: 600 }, fakeEncoder(calls), 100 * 100);
    expect(url).not.toBeNull();
    const last = calls.at(-1);
    expect(last?.width).toBeLessThan(600);
    expect(url?.length).toBeLessThanOrEqual(100 * 100);
  });

  it("refuses when even the smallest version does not fit", async () => {
    const calls: { width: number; height: number; quality: number }[] = [];
    expect(await fitImage({ width: 800, height: 800 }, fakeEncoder(calls), 10)).toBeNull();
    expect(Math.min(...calls.map((c) => c.width))).toBeGreaterThanOrEqual(64);
  });
});

describe("messageLimits", () => {
  it("uses the soft cap when the server reports no image limit", () => {
    const limits = messageLimits({
      maxMessageLength: 5000,
      maxImageMessageLength: 0,
      allowHtml: true,
    } as Parameters<typeof messageLimits>[0]);
    expect(limits).toEqual({ text: 5000, image: UNLIMITED_IMAGE_FALLBACK, allowHtml: true });
  });
});
