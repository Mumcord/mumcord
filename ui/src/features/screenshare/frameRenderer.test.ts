/** A hidden stage tab keeps taking frames but only draws the latest one once shown (FR-9.44). */
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ScreenFrame } from "../../api/screenFrames";
import { createFrameRenderer } from "./frameRenderer";

class FakeImageData {
  constructor(
    readonly data: Uint8ClampedArray,
    readonly width: number,
    readonly height: number,
  ) {}
}

function rgba(fill: number): ScreenFrame {
  return {
    kind: "rgba",
    width: 1,
    height: 1,
    data: new Uint8ClampedArray([fill, fill, fill, 255]),
  };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("frame renderer", () => {
  it("holds pictures while hidden and draws the latest when shown", () => {
    vi.stubGlobal("ImageData", FakeImageData);
    const putImageData = vi.fn();
    const canvas = {
      width: 0,
      height: 0,
      getContext: () => ({ putImageData }),
    } as unknown as HTMLCanvasElement;
    const onFirstPicture = vi.fn();
    const onFrame = vi.fn();
    const renderer = createFrameRenderer(canvas, {
      mode: "rgba",
      requestKeyframe: vi.fn(),
      onFirstPicture,
      onFrame,
    });

    renderer.setHidden(true);
    renderer.push(rgba(1));
    renderer.push(rgba(2));
    expect(putImageData).not.toHaveBeenCalled();
    // The share still counts as live and its frames as received.
    expect(onFirstPicture).toHaveBeenCalledTimes(1);
    expect(onFrame).toHaveBeenCalledTimes(2);

    renderer.setHidden(false);
    expect(putImageData).toHaveBeenCalledTimes(1);
    const drawn = putImageData.mock.calls[0]?.[0] as FakeImageData;
    expect(drawn.data[0]).toBe(2);

    renderer.push(rgba(3));
    expect(putImageData).toHaveBeenCalledTimes(2);
  });
});
