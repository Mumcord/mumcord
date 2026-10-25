import { describe, expect, it } from "vitest";
import {
  fitImage,
  MAX_IMAGE_EDGE,
  MAX_SOURCE_BYTES,
  prepareImage,
  messageLimits,
  UNLIMITED_IMAGE_FALLBACK,
  type Encoder,
  type ImageFormat,
} from "./images";

type Call = { width: number; height: number; format: ImageFormat; quality: number };

/** Fake encoder: output length grows with pixel count; PNG costs `pngCost` per pixel, JPEG scales with quality. */
function fakeEncoder(calls: Call[], pngCost: number): Encoder {
  return (width, height, format, quality) => {
    calls.push({ width, height, format, quality });
    const per = format === "png" ? pngCost : quality;
    return Promise.resolve("x".repeat(Math.round(width * height * per)));
  };
}

describe("fitImage", () => {
  it("keeps the full size when it fits, without a JPEG search", async () => {
    const calls: Call[] = [];
    const fit = await fitImage({ width: 1000, height: 500 }, fakeEncoder(calls, 1), 1e9);
    expect(fit).toMatchObject({ width: 1000, height: 500 });
    expect(calls).toHaveLength(1);
    expect(calls[0]?.format).toBe("png");
  });

  it("caps the edge at the maximum", async () => {
    const calls: Call[] = [];
    const fit = await fitImage({ width: 4000, height: 2000 }, fakeEncoder(calls, 1), 1e9);
    expect(fit).toMatchObject({ width: MAX_IMAGE_EDGE, height: MAX_IMAGE_EDGE / 2 });
  });

  it("uses the budget fully with a large edge and few attempts", async () => {
    const calls: Call[] = [];
    const budget = 800 * 800;
    const fit = await fitImage({ width: 2000, height: 2000 }, fakeEncoder(calls, 1), budget);
    expect(fit?.url.length).toBeLessThanOrEqual(budget);
    // 0.85 * edge^2 <= 640000 -> edge ~ 867 for JPEG; PNG only ~ 800.
    expect(fit?.width).toBeGreaterThan(800);
    expect(calls.length).toBeLessThanOrEqual(20);
  });

  it("prefers PNG when it fits at least as large as JPEG", async () => {
    const fit = await fitImage({ width: 1000, height: 1000 }, fakeEncoder([], 0.1), 90_000);
    expect(fit?.url.length).toBeLessThanOrEqual(90_000);
    const png = await fitImage({ width: 1000, height: 1000 }, fakeEncoder([], 0.1), 100_000);
    expect(png).toMatchObject({ width: 1000 });
  });

  it("picks JPEG when PNG would have to shrink more", async () => {
    const calls: Call[] = [];
    const fit = await fitImage({ width: 1000, height: 1000 }, fakeEncoder(calls, 3), 400_000);
    const png = Math.max(
      ...calls
        .filter((c) => c.format === "png" && 3 * c.width * c.height <= 400_000)
        .map((c) => c.width),
    );
    expect(fit?.width).toBeGreaterThan(png);
    expect(calls.some((c) => c.format === "jpeg")).toBe(true);
  });

  it("refuses when even the smallest version does not fit", async () => {
    const calls: Call[] = [];
    expect(await fitImage({ width: 800, height: 800 }, fakeEncoder(calls, 1), 10)).toBeNull();
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

describe("prepareImage size cap", () => {
  it("refuses huge files without reading them", async () => {
    const big = { type: "image/png", size: MAX_SOURCE_BYTES + 1 } as Blob;
    expect(await prepareImage(big, 1000)).toEqual({ ok: false, reason: "tooLarge" });
  });
});
