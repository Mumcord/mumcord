/**
 * Frames of a watched screen share (FR-2.44). The engine sends one binary message per frame
 * over the Tauri channel passed to `screenShareWatch`: a 16-byte little-endian header, then
 * the payload. Layout and kinds are defined in `src-tauri/src/screen_frames.rs`.
 */
import { Channel } from "@tauri-apps/api/core";

const HEADER_LEN = 16;
const KIND_VP9 = 1;
const KIND_RGBA = 2;
const FLAG_KEYFRAME = 1;

/** VP9 profile 0, level 1.0, 8 bit: what the sharer's libvpx encoder produces. */
export const VP9_CODEC = "vp09.00.10.08";

export type ScreenFrame =
  | {
      /** Encoded frame for a WebCodecs `VideoDecoder`. */
      kind: "vp9";
      keyframe: boolean;
      temporalLayer: number;
      timestampUs: number;
      width: number;
      height: number;
      data: Uint8Array;
    }
  | {
      /** Picture decoded by the engine (fallback), ready for `ImageData`. */
      kind: "rgba";
      width: number;
      height: number;
      data: Uint8ClampedArray<ArrayBuffer>;
    };

/** Reads one channel message; `null` when it is malformed or of an unknown kind. */
export function parseScreenFrame(buffer: ArrayBuffer): ScreenFrame | null {
  if (buffer.byteLength < HEADER_LEN) return null;
  const view = new DataView(buffer);
  const width = view.getUint16(12, true);
  const height = view.getUint16(14, true);
  switch (view.getUint8(0)) {
    case KIND_VP9:
      return {
        kind: "vp9",
        keyframe: (view.getUint8(1) & FLAG_KEYFRAME) !== 0,
        temporalLayer: view.getUint8(2),
        // Microseconds since the share started stay far below 2^53.
        timestampUs: Number(view.getBigUint64(4, true)),
        width,
        height,
        data: new Uint8Array(buffer, HEADER_LEN),
      };
    case KIND_RGBA:
      if (buffer.byteLength - HEADER_LEN !== width * height * 4) return null;
      return { kind: "rgba", width, height, data: new Uint8ClampedArray(buffer, HEADER_LEN) };
    default:
      return null;
  }
}

/** The channel to pass to `screenShareWatch`; malformed messages are logged and skipped. */
export function screenFrameChannel(onFrame: (frame: ScreenFrame) => void): Channel<ArrayBuffer> {
  const channel = new Channel<ArrayBuffer>();
  channel.onmessage = (message) => {
    const frame = parseScreenFrame(message);
    if (frame) onFrame(frame);
    else console.warn("skipped a malformed screen share frame", message.byteLength);
  };
  return channel;
}

/** Whether this webview decodes VP9 itself; otherwise the engine decodes (fallback). */
export async function webCodecsVp9Supported(): Promise<boolean> {
  if (typeof VideoDecoder === "undefined") return false;
  try {
    const { supported } = await VideoDecoder.isConfigSupported({ codec: VP9_CODEC });
    return supported === true;
  } catch (err) {
    console.warn("WebCodecs VP9 check failed", err);
    return false;
  }
}
