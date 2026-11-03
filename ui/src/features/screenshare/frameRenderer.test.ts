import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { VideoCodec } from "../../api";
import type { ScreenFrame } from "../../api/screenFrames";
import { createFrameRenderer } from "./frameRenderer";

/** A WebCodecs decoder stand-in that records its configuration and chunks. */
class FakeDecoder {
  static all: FakeDecoder[] = [];
  state = "unconfigured";
  codec = "";
  chunks: string[] = [];
  constructor(
    readonly init: { output: (picture: unknown) => void; error: (error: unknown) => void },
  ) {
    FakeDecoder.all.push(this);
  }
  configure({ codec }: { codec: string }) {
    this.codec = codec;
    this.state = "configured";
  }
  decode(chunk: { type: string }) {
    this.chunks.push(chunk.type);
  }
  close() {
    this.state = "closed";
  }
  /** Emits one decoded picture. */
  picture() {
    this.init.output({ displayWidth: 64, displayHeight: 32, close: () => undefined });
  }
}

function encoded(codec: VideoCodec, keyframe: boolean, width = 64): ScreenFrame {
  return {
    kind: "encoded",
    codec,
    keyframe,
    temporalLayer: 0,
    timestampUs: 0,
    width,
    height: 32,
    data: new Uint8Array([1]),
  };
}

function rgba(codec: VideoCodec): ScreenFrame {
  return { kind: "rgba", codec, width: 1, height: 1, data: new Uint8ClampedArray(4) };
}

function canvas() {
  const context = { drawImage: vi.fn(), putImageData: vi.fn() };
  const element = { width: 0, height: 0, getContext: () => context };
  return { element: element as unknown as HTMLCanvasElement, context };
}

describe("frame renderer", () => {
  beforeEach(() => {
    FakeDecoder.all = [];
    vi.stubGlobal("VideoDecoder", FakeDecoder);
    vi.stubGlobal(
      "EncodedVideoChunk",
      class {
        type: string;
        constructor({ type }: { type: string }) {
          this.type = type;
        }
      },
    );
    vi.stubGlobal(
      "ImageData",
      class {
        constructor(
          readonly data: Uint8ClampedArray,
          readonly width: number,
        ) {}
      },
    );
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("configures WebCodecs for the frame's codec and switches at a keyframe", () => {
    const requestKeyframe = vi.fn();
    const renderer = createFrameRenderer(canvas().element, { requestKeyframe });
    renderer.push(encoded("av1", true));
    renderer.push(encoded("av1", false));
    expect(FakeDecoder.all).toHaveLength(1);
    expect(FakeDecoder.all[0]?.codec).toBe("av01.0.13M.08");
    expect(FakeDecoder.all[0]?.chunks).toEqual(["key", "delta"]);

    // The sharer switched to VP9 for an old viewer: the delta before its keyframe waits.
    renderer.push(encoded("vp9", false));
    expect(requestKeyframe).toHaveBeenCalledTimes(1);
    renderer.push(encoded("vp9", true));
    expect(FakeDecoder.all).toHaveLength(2);
    expect(FakeDecoder.all[0]?.state).toBe("closed");
    expect(FakeDecoder.all[1]?.codec).toBe("vp09.00.10.08");
  });

  it("draws engine pictures and encoded frames in one watch", () => {
    const { element, context } = canvas();
    const onFirstPicture = vi.fn();
    const renderer = createFrameRenderer(element, { requestKeyframe: vi.fn(), onFirstPicture });
    renderer.push(rgba("av1"));
    expect(context.putImageData).toHaveBeenCalledTimes(1);
    expect(onFirstPicture).toHaveBeenCalledTimes(1);
    renderer.push(encoded("vp9", true));
    FakeDecoder.all[0]?.picture();
    expect(context.drawImage).toHaveBeenCalledTimes(1);
  });

  it("reports a codec WebCodecs fails on before its first picture", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const onUnsupported = vi.fn();
    const renderer = createFrameRenderer(canvas().element, {
      requestKeyframe: vi.fn(),
      onUnsupported,
    });
    renderer.push(encoded("av1", true));
    FakeDecoder.all[0]?.init.error(new Error("unsupported"));
    expect(onUnsupported).toHaveBeenCalledWith("av1");

    // After pictures, an error is a broken frame, not a missing codec.
    renderer.push(encoded("vp9", true));
    FakeDecoder.all[1]?.picture();
    FakeDecoder.all[1]?.init.error(new Error("corrupt"));
    expect(onUnsupported).toHaveBeenCalledTimes(1);
    warn.mockRestore();
  });
});
