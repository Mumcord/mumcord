/**
 * Frames of a watched screen share (FR-2.44). The engine sends one binary message per frame
 * over the Tauri channel passed to `screenShareWatch`: a 16-byte little-endian header, then
 * the payload. Layout and kinds are defined in `src-tauri/src/screen_frames.rs`.
 */
import { Channel } from "@tauri-apps/api/core";
import type { VideoCodec } from "../bindings/commands";

const HEADER_LEN = 16;
const KIND_ENCODED = 1;
const KIND_RGBA = 2;
const FLAG_KEYFRAME = 1;
const CODEC_BYTES: Record<number, VideoCodec> = { 1: "vp9", 2: "av1" };

/** WebCodecs strings per codec, as the sharer's encoders produce them (8 bit). */
export const WEBCODECS_CODEC: Record<VideoCodec, string> = {
  /** VP9 profile 0, level 1.0. */
  vp9: "vp09.00.10.08",
  /** AV1 main profile, level 5.1, main tier: up to 4K at 60 fps, so every quality step fits. */
  av1: "av01.0.13M.08",
};

export const VIDEO_CODECS: readonly VideoCodec[] = ["av1", "vp9"];

export type ScreenFrame =
  | {
      /** Encoded frame for a WebCodecs `VideoDecoder`. */
      kind: "encoded";
      codec: VideoCodec;
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
      codec: VideoCodec;
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
  const codec = CODEC_BYTES[view.getUint8(3)];
  if (codec === undefined) return null;
  switch (view.getUint8(0)) {
    case KIND_ENCODED:
      return {
        kind: "encoded",
        codec,
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
      return {
        kind: "rgba",
        codec,
        width,
        height,
        data: new Uint8ClampedArray(buffer, HEADER_LEN),
      };
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

/** Whether this webview decodes `codec` itself; otherwise the engine decodes it (fallback). */
async function webCodecsSupports(codec: VideoCodec): Promise<boolean> {
  if (typeof VideoDecoder === "undefined") return false;
  try {
    const { supported } = await VideoDecoder.isConfigSupported({ codec: WEBCODECS_CODEC[codec] });
    return supported === true;
  } catch (err) {
    console.warn("WebCodecs check failed", codec, err);
    return false;
  }
}

/** The codecs this webview decodes with WebCodecs (FR-7.59). */
export async function webCodecsCodecs(): Promise<VideoCodec[]> {
  const supported = await Promise.all(VIDEO_CODECS.map(webCodecsSupports));
  return VIDEO_CODECS.filter((_, i) => supported[i]);
}
