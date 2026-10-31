/**
 * Helpers of the inline audio player (issue #260): time and size labels, turning the engine's
 * `data:` URL into a blob the `<audio>` element plays, and the waveform drawn from the file.
 */

/** Bars of the waveform; fits the card's width at the smallest scaling. */
export const WAVEFORM_BARS = 56;

/** `m:ss`, or `h:mm:ss` from an hour on; `null` for an unknown length. */
export function formatTime(seconds: number | null): string {
  if (seconds === null || !Number.isFinite(seconds) || seconds < 0) return "–:––";
  const total = Math.floor(seconds);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, "0");
  return h > 0 ? `${String(h)}:${String(m).padStart(2, "0")}:${s}` : `${String(m)}:${s}`;
}

/** File size in the user's language: kB below one megabyte, MB with one decimal above. */
export function formatSize(bytes: number, language: string): string {
  const mb = bytes >= 1_000_000;
  return new Intl.NumberFormat(language, {
    style: "unit",
    unit: mb ? "megabyte" : "kilobyte",
    maximumFractionDigits: mb ? 1 : 0,
  }).format(mb ? bytes / 1_000_000 : Math.max(1, bytes / 1000));
}

/** The bytes of a base64 `data:` URL as a blob of its media type; `null` when malformed. */
export function dataUrlToBlob(dataUrl: string): Blob | null {
  const match = /^data:([a-z0-9.+/-]+);base64,/.exec(dataUrl);
  const type = match?.[1];
  if (match === null || type === undefined) return null;
  let binary: string;
  try {
    binary = atob(dataUrl.slice(match[0].length));
  } catch {
    return null;
  }
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return new Blob([bytes], { type });
}

/** Loudest sample per bar, scaled so the loudest bar is 1; silence stays flat. */
export function peaksOf(samples: Float32Array, bars = WAVEFORM_BARS): number[] {
  const size = Math.max(1, Math.floor(samples.length / bars));
  const peaks: number[] = [];
  for (let bar = 0; bar < bars; bar++) {
    let peak = 0;
    const end = Math.min(samples.length, (bar + 1) * size);
    for (let i = bar * size; i < end; i++) peak = Math.max(peak, Math.abs(samples[i] ?? 0));
    peaks.push(peak);
  }
  const loudest = Math.max(...peaks);
  return loudest > 0 ? peaks.map((p) => p / loudest) : peaks;
}

/**
 * The waveform of an audio file, decoded offline (nothing is played and no output device is
 * opened); `null` when the web view cannot decode it, which leaves the player without bars.
 */
export async function waveformOf(blob: Blob): Promise<number[] | null> {
  if (typeof OfflineAudioContext === "undefined") return null;
  try {
    const context = new OfflineAudioContext(1, 1, 44_100);
    const decoded = await context.decodeAudioData(await blob.arrayBuffer());
    return peaksOf(decoded.getChannelData(0));
  } catch (error: unknown) {
    console.warn("No waveform for an audio file", error);
    return null;
  }
}
