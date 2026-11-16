import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { parseScreenFrame, screenFrameChannel, webCodecsCodecs } from "./screenFrames";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(null),
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
  codec = 1,
) {
  const buffer = new ArrayBuffer(16 + payload.length);
  const view = new DataView(buffer);
  view.setUint8(0, kind);
  view.setUint8(1, flags);
  view.setUint8(2, layer);
  view.setUint8(3, codec);
  view.setBigUint64(4, 1_234_567n, true);
  view.setUint16(12, w, true);
  view.setUint16(14, h, true);
  new Uint8Array(buffer, 16).set(payload);
  return buffer;
}

describe("screen share frames", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.mocked(invoke).mockClear();
  });

  it("reads an encoded VP9 frame", () => {
    expect(parseScreenFrame(message(1, 1, 2, 1280, 720, [0xaa, 0xbb]))).toEqual({
      kind: "encoded",
      codec: "vp9",
      keyframe: true,
      temporalLayer: 2,
      timestampUs: 1_234_567,
      width: 1280,
      height: 720,
      data: new Uint8Array([0xaa, 0xbb]),
    });
    const delta = parseScreenFrame(message(1, 0, 1, 1280, 720, [1]));
    expect(delta?.kind === "encoded" && delta.keyframe).toBe(false);
  });

  it("reads the codec of encoded frames and of pictures", () => {
    expect(parseScreenFrame(message(1, 0, 3, 64, 64, [1], 2))).toMatchObject({
      kind: "encoded",
      codec: "av1",
      temporalLayer: 3,
    });
    expect(parseScreenFrame(message(2, 0, 0, 1, 1, [1, 2, 3, 255], 2))).toMatchObject({
      kind: "rgba",
      codec: "av1",
    });
  });

  it("reads an RGBA picture of the stated size", () => {
    const frame = parseScreenFrame(message(2, 0, 0, 1, 2, [1, 2, 3, 255, 4, 5, 6, 255]));
    expect(frame).toEqual({
      kind: "rgba",
      codec: "vp9",
      width: 1,
      height: 2,
      data: new Uint8ClampedArray([1, 2, 3, 255, 4, 5, 6, 255]),
    });
  });

  it("rejects short, mis-sized and unknown messages", () => {
    expect(parseScreenFrame(new ArrayBuffer(8))).toBeNull();
    expect(parseScreenFrame(message(2, 0, 0, 2, 2, [1, 2, 3]))).toBeNull();
    expect(parseScreenFrame(message(9, 0, 0, 1, 1, []))).toBeNull();
    expect(parseScreenFrame(message(1, 0, 0, 1, 1, [], 7))).toBeNull();
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

  it("acknowledges only credit-tagged RGBA frames after their handler consumes them", () => {
    const onFrame = vi.fn(() => {
      expect(invoke).not.toHaveBeenCalled();
    });
    const channel = screenFrameChannel(onFrame);
    channel.onmessage(message(2, 2, 0, 1, 1, [1, 2, 3, 255]));
    expect(invoke).toHaveBeenCalledExactlyOnceWith("screen_frame_ack", { token: "1234567" });
    vi.mocked(invoke).mockClear();
    channel.onmessage(message(2, 0, 0, 1, 1, [1, 2, 3, 255]));
    channel.onmessage(message(1, 2, 0, 1, 1, [7]));
    expect(invoke).not.toHaveBeenCalled();
  });

  it("drops a late older picture but releases both credits without losing 64-bit token precision", () => {
    const onFrame = vi.fn();
    const channel = screenFrameChannel(onFrame);
    for (const token of [9_007_199_254_740_993n, 9_007_199_254_740_992n]) {
      const buffer = message(2, 2, 0, 1, 1, [1, 2, 3, 255]);
      new DataView(buffer).setBigUint64(4, token, true);
      channel.onmessage(buffer);
    }
    expect(onFrame).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenNthCalledWith(1, "screen_frame_ack", { token: "9007199254740993" });
    expect(invoke).toHaveBeenNthCalledWith(2, "screen_frame_ack", { token: "9007199254740992" });
  });

  it("releases credit even if drawing throws, and reports acknowledgement errors", async () => {
    const error = new Error("draw failed");
    const channel = screenFrameChannel(() => {
      throw error;
    });
    expect(() => {
      channel.onmessage(message(2, 2, 0, 1, 1, [1, 2, 3, 255]));
    }).toThrow(error);
    expect(invoke).toHaveBeenCalledOnce();
    vi.mocked(invoke).mockRejectedValueOnce(new Error("credit failed"));
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    screenFrameChannel(() => undefined).onmessage(message(2, 2, 0, 1, 1, [1, 2, 3, 255]));
    await Promise.resolve();
    expect(warn).toHaveBeenCalledWith("screen frame acknowledgement failed", expect.any(Error));
    warn.mockRestore();
  });

  it("detects the codecs WebCodecs decodes", async () => {
    expect(await webCodecsCodecs()).toEqual([]);
    const isConfigSupported = vi.fn().mockResolvedValue({ supported: true });
    vi.stubGlobal("VideoDecoder", { isConfigSupported });
    expect(await webCodecsCodecs()).toEqual(["av1", "vp9"]);
    expect(isConfigSupported).toHaveBeenCalledWith({ codec: "av01.0.13M.08" });
    expect(isConfigSupported).toHaveBeenCalledWith({ codec: "vp09.00.10.08" });
    // A webview without `av01` (AC-7.41) leaves AV1 to the engine.
    isConfigSupported.mockImplementation(({ codec }: { codec: string }) =>
      Promise.resolve({ supported: codec.startsWith("vp09") }),
    );
    expect(await webCodecsCodecs()).toEqual(["vp9"]);
    isConfigSupported.mockRejectedValue(new Error("no"));
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    expect(await webCodecsCodecs()).toEqual([]);
    warn.mockRestore();
  });
});
