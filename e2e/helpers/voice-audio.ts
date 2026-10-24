// WAV helpers for the voice specs: build input clips for the app's file audio backend and measure
// what the bot recorded or the app played (48 kHz mono 16-bit PCM throughout).
import { readFileSync, statSync, writeFileSync } from "node:fs";

export const RATE = 48_000;

/** Reads a 16-bit PCM WAV as floats. Uses the file length, not the header's data size, so a
 * file that is still being written (the app's output, header refreshed once a second) works. */
export function readWav(path: string): Float32Array {
  const buf = readFileSync(path);
  let off = 12;
  while (off + 8 <= buf.length) {
    const id = buf.toString("ascii", off, off + 4);
    const size = buf.readUInt32LE(off + 4);
    if (id === "data") {
      const start = off + 8;
      const n = Math.floor((buf.length - start) / 2);
      const out = new Float32Array(n);
      for (let i = 0; i < n; i++) out[i] = buf.readInt16LE(start + i * 2) / 32768;
      return out;
    }
    off += 8 + size + (size % 2);
  }
  throw new Error(`no data chunk in ${path}`);
}

/** Number of samples currently in a growing WAV file (44-byte PCM header assumed). */
export function wavSamples(path: string): number {
  try {
    return Math.max(0, Math.floor((statSync(path).size - 44) / 2));
  } catch {
    return 0;
  }
}

export function writeWav(path: string, samples: Float32Array): void {
  const buf = Buffer.alloc(44 + samples.length * 2);
  buf.write("RIFF", 0, "ascii");
  buf.writeUInt32LE(36 + samples.length * 2, 4);
  buf.write("WAVE", 8, "ascii");
  buf.write("fmt ", 12, "ascii");
  buf.writeUInt32LE(16, 16);
  buf.writeUInt16LE(1, 20);
  buf.writeUInt16LE(1, 22);
  buf.writeUInt32LE(RATE, 24);
  buf.writeUInt32LE(RATE * 2, 28);
  buf.writeUInt16LE(2, 32);
  buf.writeUInt16LE(16, 34);
  buf.write("data", 36, "ascii");
  buf.writeUInt32LE(samples.length * 2, 40);
  for (let i = 0; i < samples.length; i++) {
    const v = Math.max(-1, Math.min(1, samples[i] ?? 0));
    buf.writeInt16LE(Math.round(v * 32767), 44 + i * 2);
  }
  writeFileSync(path, buf);
}

export function silence(seconds: number): Float32Array {
  return new Float32Array(Math.round(seconds * RATE));
}

export function concat(...parts: Float32Array[]): Float32Array {
  const out = new Float32Array(parts.reduce((n, p) => n + p.length, 0));
  let off = 0;
  for (const p of parts) {
    out.set(p, off);
    off += p.length;
  }
  return out;
}

export function db(rms: number): number {
  return rms > 0 ? 20 * Math.log10(rms) : -200;
}

export function rms(x: Float32Array, from = 0, to = x.length): number {
  let s = 0;
  const a = Math.max(0, from);
  const b = Math.min(x.length, to);
  for (let i = a; i < b; i++) s += (x[i] ?? 0) ** 2;
  return b > a ? Math.sqrt(s / (b - a)) : 0;
}

/** RMS in dBFS per window of `ms` milliseconds. */
export function levels(x: Float32Array, ms = 100): number[] {
  const w = Math.round((RATE * ms) / 1000);
  const out: number[] = [];
  for (let i = 0; i + w <= x.length; i += w) out.push(db(rms(x, i, i + w)));
  return out;
}

/** Windows (of `ms`) at or above `minDb`: first/last index and count. */
export function activity(
  x: Float32Array,
  minDb: number,
  ms = 100,
): { first: number; last: number; count: number; total: number } {
  const lv = levels(x, ms);
  let first = -1;
  let last = -1;
  let count = 0;
  lv.forEach((l, i) => {
    if (l >= minDb) {
      if (first < 0) first = i;
      last = i;
      count++;
    }
  });
  return { first, last, count, total: lv.length };
}

/** Mean power (as dBFS) over the windows whose level is at least `minDb`. */
export function activeLevel(x: Float32Array, minDb: number, ms = 100): number {
  const w = Math.round((RATE * ms) / 1000);
  let s = 0;
  let n = 0;
  for (let i = 0; i + w <= x.length; i += w) {
    const r = rms(x, i, i + w);
    if (db(r) >= minDb) {
      s += r * r;
      n++;
    }
  }
  return n > 0 ? db(Math.sqrt(s / n)) : -200;
}

/** Envelope (linear RMS per 10 ms window). */
function envelope(x: Float32Array): number[] {
  const w = RATE / 100;
  const out: number[] = [];
  for (let i = 0; i + w <= x.length; i += w) out.push(rms(x, i, i + w));
  return out;
}

/** Offset (in 10 ms windows) of `ref` inside `rec` that maximises envelope correlation. */
export function align(rec: Float32Array, ref: Float32Array): number {
  const a = envelope(rec);
  const b = envelope(ref);
  let best = 0;
  let bestScore = -Infinity;
  for (let lag = 0; lag + b.length <= a.length; lag++) {
    let s = 0;
    for (let i = 0; i < b.length; i++) s += (a[lag + i] ?? 0) * (b[i] ?? 0);
    if (s > bestScore) {
      bestScore = s;
      best = lag;
    }
  }
  return best;
}

/**
 * Speech-vs-gap levels of `sig` (aligned to the clean reference at window offset `lag`, 10 ms
 * windows): speech = windows where the clean clip is loud, gap = where it is silent.
 * `skipFirst` ignores the first windows of the clip (onset transient after digital silence).
 */
export function speechAndGap(
  sig: Float32Array,
  clean: Float32Array,
  lag: number,
  skipFirst = 0,
): { speechDb: number; gapDb: number; gapWindows: number; gapSentWindows: number } {
  const ref = envelope(clean);
  const env = envelope(sig);
  let sp = 0;
  let spn = 0;
  let gp = 0;
  let gpn = 0;
  let sent = 0;
  ref.forEach((r, i) => {
    const v = env[lag + i] ?? 0;
    const rdb = db(r);
    if (rdb >= -30) {
      sp += v * v;
      spn++;
    } else if (rdb <= -90 && i >= skipFirst) {
      // Skip the first/last 30 ms of a gap: codec and suppressor tails.
      const near = [-3, -2, -1, 1, 2, 3].some((d) => db(ref[i + d] ?? 0) > -90);
      if (near) return;
      gp += v * v;
      gpn++;
      if (v > 0) sent++;
    }
  });
  return {
    speechDb: db(Math.sqrt(sp / Math.max(1, spn))),
    gapDb: db(Math.sqrt(gp / Math.max(1, gpn))),
    gapWindows: gpn,
    gapSentWindows: sent,
  };
}
