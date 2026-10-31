/**
 * What a watched share looks like, counted from the frames themselves (issue #163, FR-3.126): the
 * picture size, the frame rate, the bitrate, the frames this viewer lost, and how far the last frame
 * lagged behind the fastest one in the window. The real one-way delay cannot be known from the frames
 * alone, because the sharer's clock is not here, so the UI shows the difference between frames.
 */

/** Frames this long ago still count. */
export const WINDOW_MS = 1000;

export interface FrameSample {
  /** `Date.now()` when the frame arrived. */
  atMs: number;
  /** Microseconds since the share started, as the sharer stamped them. */
  timestampUs: number;
  /** Payload bytes. */
  bytes: number;
  width: number;
  height: number;
}

export interface StreamStats {
  width: number;
  height: number;
  /** Video frames in the last second. */
  fps: number;
  /** Payload bytes in the last second, as kilobit per second. */
  kbps: number;
  /** Frames this viewer dropped in total (a full queue or a decoder failure). */
  dropped: number;
  /** How much later the last video frame came than the fastest one, in milliseconds. */
  delayMs: number | null;
}

/** Counts the frames of one watch. */
export class StreamMeter {
  private video: FrameSample[] = [];
  private dropped = 0;

  /** A video frame arrived. */
  frame(sample: FrameSample): void {
    this.video.push(sample);
    this.prune(sample.atMs);
  }

  /** A frame was lost (the queue was full or the decoder failed). */
  drop(atMs: number): void {
    this.dropped += 1;
    this.prune(atMs);
  }

  snapshot(): StreamStats {
    const last = this.video[this.video.length - 1];
    const bytes = this.video.reduce((sum, sample) => sum + sample.bytes, 0);
    return {
      width: last?.width ?? 0,
      height: last?.height ?? 0,
      fps: this.video.length,
      kbps: Math.round((bytes * 8) / WINDOW_MS),
      dropped: this.dropped,
      delayMs: last === undefined ? null : this.delayOf(last),
    };
  }

  /** The delay of `sample` against the fastest frame in the window. */
  private delayOf(sample: FrameSample): number | null {
    let best = Number.POSITIVE_INFINITY;
    for (const other of this.video) best = Math.min(best, gap(other));
    return Number.isFinite(best) ? Math.max(0, Math.round(gap(sample) - best)) : null;
  }

  private prune(now: number): void {
    this.video = this.video.filter((sample) => sample.atMs > now - WINDOW_MS);
  }
}

/** How long a frame took from its capture stamp to arrival, in milliseconds. */
function gap(sample: FrameSample): number {
  return sample.atMs - Math.round(sample.timestampUs / 1000);
}
