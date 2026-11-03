/**
 * Draws the frames of a watched screen share onto a canvas (FR-2.44). VP9 frames go through a
 * WebCodecs `VideoDecoder`; RGBA pictures (the engine's fallback decode) are drawn directly.
 * The decoder only starts on a keyframe: delta frames before it cannot be decoded, so they are
 * dropped and a keyframe is asked for. A decoder error closes the decoder; the next keyframe
 * starts a fresh one.
 */
import { VP9_CODEC, type ScreenFrame } from "../../api/screenFrames";

export interface FrameRenderer {
  /** Takes one frame from the channel; frames of the other kind are ignored. */
  push(frame: ScreenFrame): void;
  /**
   * While hidden (another stage tab is the main item, FR-9.44) frames are still taken and decoded,
   * but only the latest picture is kept; it is drawn once the canvas shows again.
   */
  setHidden(hidden: boolean): void;
  /** Frees the decoder; later frames are ignored. */
  close(): void;
}

export interface RendererOptions {
  /** `"vp9"` decodes in the webview, `"rgba"` draws the engine's decoded pictures. */
  mode: "vp9" | "rgba";
  /** Asks the sharer for a keyframe; called once per wait, not for every dropped frame. */
  requestKeyframe: () => void;
  /** The first picture is on the canvas. */
  onFirstPicture?: () => void;
  /** A frame arrived, for the frame rate, the bitrate and the delay (issue #163). */
  onFrame?: (frame: ScreenFrame, atMs: number) => void;
  /** A frame was dropped. */
  onDrop?: (atMs: number) => void;
}

export function createFrameRenderer(
  canvas: HTMLCanvasElement,
  { mode, requestKeyframe, onFirstPicture, onFrame, onDrop }: RendererOptions,
): FrameRenderer {
  const context = canvas.getContext("2d");
  let closed = false;
  let pictured = false;
  let hidden = false;
  /** The latest picture that came while hidden, drawn when the canvas shows again. */
  let held: VideoFrame | Extract<ScreenFrame, { kind: "rgba" }> | null = null;

  function firstPicture(): void {
    if (pictured) return;
    pictured = true;
    onFirstPicture?.();
  }

  function draw(source: CanvasImageSource | ImageData, width: number, height: number): void {
    if (closed || context === null) return;
    if (canvas.width !== width) canvas.width = width;
    if (canvas.height !== height) canvas.height = height;
    if (source instanceof ImageData) context.putImageData(source, 0, 0);
    else context.drawImage(source, 0, 0, width, height);
    firstPicture();
  }

  function drawRgba(frame: Extract<ScreenFrame, { kind: "rgba" }>): void {
    draw(new ImageData(frame.data, frame.width, frame.height), frame.width, frame.height);
  }

  /** Keeps the picture instead of drawing it; the share still counts as live. */
  function hold(picture: NonNullable<typeof held>): void {
    if (held !== null && held !== picture && "close" in held) held.close();
    held = picture;
    firstPicture();
  }

  function releaseHeld(): void {
    const picture = held;
    held = null;
    if (picture === null) return;
    if ("close" in picture) {
      draw(picture, picture.displayWidth, picture.displayHeight);
      picture.close();
    } else drawRgba(picture);
  }

  function setHidden(next: boolean): void {
    hidden = next;
    if (!hidden) releaseHeld();
  }

  if (context === null) console.error("screen share canvas has no 2d context");

  if (mode === "rgba") {
    return {
      push(frame) {
        if (frame.kind !== "rgba") return;
        onFrame?.(frame, Date.now());
        if (hidden) hold(frame);
        else drawRgba(frame);
      },
      setHidden,
      close() {
        closed = true;
        held = null;
      },
    };
  }

  let decoder: VideoDecoder | null = null;
  let size = "";
  let waitingForKeyframe = false;

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

  function startDecoder(width: number, height: number): VideoDecoder {
    const next = new VideoDecoder({
      output: (picture) => {
        if (hidden && !closed) {
          hold(picture);
          return;
        }
        draw(picture, picture.displayWidth, picture.displayHeight);
        picture.close();
      },
      error: (error) => {
        console.warn("screen share decoder failed", error);
        if (decoder === next) decoder = null;
        waitForKeyframe();
      },
    });
    next.configure({
      codec: VP9_CODEC,
      codedWidth: width,
      codedHeight: height,
      optimizeForLatency: true,
    });
    return next;
  }

  return {
    push(frame) {
      if (closed || frame.kind !== "vp9") return;
      onFrame?.(frame, Date.now());
      const frameSize = `${String(frame.width)}x${String(frame.height)}`;
      if (frame.keyframe) {
        // A new size (the quality step changed the resolution) needs a newly configured decoder.
        if (decoder === null || size !== frameSize) {
          dropDecoder();
          decoder = startDecoder(frame.width, frame.height);
          size = frameSize;
        }
        waitingForKeyframe = false;
      } else if (decoder === null) {
        waitForKeyframe();
        return;
      }
      try {
        decoder.decode(
          new EncodedVideoChunk({
            type: frame.keyframe ? "key" : "delta",
            timestamp: frame.timestampUs,
            data: frame.data,
          }),
        );
      } catch (error) {
        console.warn("screen share frame could not be decoded", error);
        dropDecoder();
        waitForKeyframe();
      }
    },
    setHidden,
    close() {
      closed = true;
      if (held !== null && "close" in held) held.close();
      held = null;
      dropDecoder();
    },
  };
}
