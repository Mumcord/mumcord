/**
 * Draws the frames of a watched screen share onto a canvas (FR-2.44, FR-7.59). Encoded AV1 and
 * VP9 frames go through a WebCodecs `VideoDecoder`; RGBA pictures (the engine's decode, for a
 * codec the webview cannot decode) are drawn directly. Both can arrive in one watch, since the
 * sharer may switch codecs mid-share. The decoder only starts on a keyframe: delta frames before
 * it cannot be decoded, so they are dropped and a keyframe is asked for. A decoder error closes
 * the decoder; the next keyframe starts a fresh one. A codec that fails before its first picture
 * is reported as unsupported, so the engine can take it over.
 */
import type { VideoCodec } from "../../api";
import { WEBCODECS_CODEC, type ScreenFrame } from "../../api/screenFrames";

export interface FrameRenderer {
  /** Takes one frame from the channel. */
  push(frame: ScreenFrame): void;
  /** Frees the decoder; later frames are ignored. */
  close(): void;
}

export interface RendererOptions {
  /** Asks the sharer for a keyframe; called once per wait, not for every dropped frame. */
  requestKeyframe: () => void;
  /** The first picture is on the canvas. */
  onFirstPicture?: () => void;
  /** A frame arrived, for the frame rate, the bitrate and the delay (issue #163). */
  onFrame?: (frame: ScreenFrame, atMs: number) => void;
  /** A frame was dropped. */
  onDrop?: (atMs: number) => void;
  /** WebCodecs failed on `codec` before showing any picture of it: the engine should decode it. */
  onUnsupported?: (codec: VideoCodec) => void;
}

export function createFrameRenderer(
  canvas: HTMLCanvasElement,
  { requestKeyframe, onFirstPicture, onFrame, onDrop, onUnsupported }: RendererOptions,
): FrameRenderer {
  const context = canvas.getContext("2d");
  let closed = false;
  let pictured = false;

  function draw(source: CanvasImageSource | ImageData, width: number, height: number): void {
    if (closed || context === null) return;
    if (canvas.width !== width) canvas.width = width;
    if (canvas.height !== height) canvas.height = height;
    if (source instanceof ImageData) context.putImageData(source, 0, 0);
    else context.drawImage(source, 0, 0, width, height);
    if (!pictured) {
      pictured = true;
      onFirstPicture?.();
    }
  }

  if (context === null) console.error("screen share canvas has no 2d context");

  let decoder: VideoDecoder | null = null;
  /** Codec and size the decoder is configured for, e.g. "av1 1920x1080". */
  let config = "";
  let waitingForKeyframe = false;
  /** Codecs WebCodecs has shown a picture of in this watch. */
  const decoded = new Set<VideoCodec>();

  function waitForKeyframe(): void {
    if (waitingForKeyframe || closed) return;
    waitingForKeyframe = true;
    onDrop?.(Date.now());
    requestKeyframe();
  }

  function dropDecoder(): void {
    if (decoder !== null && decoder.state !== "closed") decoder.close();
    decoder = null;
  }

  function failed(codec: VideoCodec, error: unknown): void {
    console.warn("screen share decoder failed", codec, error);
    if (!decoded.has(codec)) onUnsupported?.(codec);
    waitForKeyframe();
  }

  function startDecoder(codec: VideoCodec, width: number, height: number): VideoDecoder {
    const next = new VideoDecoder({
      output: (picture) => {
        decoded.add(codec);
        draw(picture, picture.displayWidth, picture.displayHeight);
        picture.close();
      },
      error: (error) => {
        if (decoder === next) decoder = null;
        failed(codec, error);
      },
    });
    next.configure({
      codec: WEBCODECS_CODEC[codec],
      codedWidth: width,
      codedHeight: height,
      optimizeForLatency: true,
    });
    return next;
  }

  function pushEncoded(frame: Extract<ScreenFrame, { kind: "encoded" }>): void {
    const frameConfig = `${frame.codec} ${String(frame.width)}x${String(frame.height)}`;
    if (!frame.keyframe && (decoder === null || config !== frameConfig)) {
      waitForKeyframe();
      return;
    }
    try {
      // A new codec or size (the sharer switched codec or quality step) needs a new decoder.
      if (decoder === null || config !== frameConfig) {
        dropDecoder();
        config = frameConfig;
        decoder = startDecoder(frame.codec, frame.width, frame.height);
      }
      waitingForKeyframe = false;
      decoder.decode(
        new EncodedVideoChunk({
          type: frame.keyframe ? "key" : "delta",
          timestamp: frame.timestampUs,
          data: frame.data,
        }),
      );
    } catch (error) {
      dropDecoder();
      failed(frame.codec, error);
    }
  }

  return {
    push(frame) {
      if (closed) return;
      onFrame?.(frame, Date.now());
      if (frame.kind === "rgba") {
        // The engine decodes this codec now; a later switch back starts at a keyframe.
        dropDecoder();
        config = "";
        draw(new ImageData(frame.data, frame.width, frame.height), frame.width, frame.height);
      } else {
        pushEncoded(frame);
      }
    },
    close() {
      closed = true;
      dropDecoder();
    },
  };
}
