import { afterEach, describe, expect, it, vi } from "vitest";
import { parseScreenFrame, screenFrameChannel, webCodecsVp9Supported } from "./screenFrames";

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage: (message: ArrayBuffer) => void = () => undefined;
  },
}));

function message(
  kind: number,
  flags: number,
  layer: number,
  w: number,
  h: number,
  payload: number[],
) {
  const buffer = new ArrayBuffer(16 + payload.length);
  const view = new DataView(buffer);
  view.setUint8(0, kind);
  view.setUint8(1, flags);
  view.setUint8(2, layer);
  view.setBigUint64(4, 1_234_567n, true);
  view.setUint16(12, w, true);
  view.setUint16(14, h, true);
  new Uint8Array(buffer, 16).set(payload);
  return buffer;
}

describe("screen share frames", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("reads an encoded VP9 frame", () => {
    expect(parseScreenFrame(message(1, 1, 2, 1280, 720, [0xaa, 0xbb]))).toEqual({
      kind: "vp9",
      keyframe: true,
      temporalLayer: 2,
      timestampUs: 1_234_567,
      width: 1280,
      height: 720,
      data: new Uint8Array([0xaa, 0xbb]),
    });
    const delta = parseScreenFrame(message(1, 0, 1, 1280, 720, [1]));
    expect(delta?.kind === "vp9" && delta.keyframe).toBe(false);
  });

  it("reads an RGBA picture of the stated size", () => {
    const frame = parseScreenFrame(message(2, 0, 0, 1, 2, [1, 2, 3, 255, 4, 5, 6, 255]));
    expect(frame).toEqual({
      kind: "rgba",
      width: 1,
      height: 2,
      data: new Uint8ClampedArray([1, 2, 3, 255, 4, 5, 6, 255]),
    });
  });

  it("rejects short, mis-sized and unknown messages", () => {
    expect(parseScreenFrame(new ArrayBuffer(8))).toBeNull();
    expect(parseScreenFrame(message(2, 0, 0, 2, 2, [1, 2, 3]))).toBeNull();
    expect(parseScreenFrame(message(9, 0, 0, 1, 1, []))).toBeNull();
  });

  it("hands parsed frames to the handler and skips malformed ones", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const onFrame = vi.fn();
    const channel = screenFrameChannel(onFrame);
    channel.onmessage(message(1, 1, 0, 2, 2, [7]));
    channel.onmessage(new ArrayBuffer(3));
    expect(onFrame).toHaveBeenCalledTimes(1);
    expect(warn).toHaveBeenCalledTimes(1);
    warn.mockRestore();
  });

  it("detects WebCodecs VP9 support", async () => {
    expect(await webCodecsVp9Supported()).toBe(false);
    const isConfigSupported = vi.fn().mockResolvedValue({ supported: true });
    vi.stubGlobal("VideoDecoder", { isConfigSupported });
    expect(await webCodecsVp9Supported()).toBe(true);
    expect(isConfigSupported).toHaveBeenCalledWith({ codec: "vp09.00.10.08" });
    isConfigSupported.mockResolvedValue({ supported: false });
    expect(await webCodecsVp9Supported()).toBe(false);
  });
});
