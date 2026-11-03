/**
 * Formatting for the file cards (FR-7.19 – 7.29): sizes, progress, the file type label and the
 * share's end time. Sizes use 1024 steps with one decimal, like the plain text line the engine
 * writes for official Mumble clients, so both always show the same number.
 */
import type { FilesMaxSize } from "../../../api";

const UNITS = ["KB", "MB", "GB", "TB"] as const;

interface Scaled {
  value: number;
  unit: string;
  divisor: number;
}

function scale(bytes: number): Scaled {
  if (bytes < 1024) return { value: bytes, unit: "B", divisor: 1 };
  let divisor = 1024;
  let unit = 0;
  while (bytes / divisor >= 1024 && unit < UNITS.length - 1) {
    divisor *= 1024;
    unit += 1;
  }
  return { value: bytes / divisor, unit: UNITS[unit] ?? "TB", divisor };
}

function number(value: number, language: string, decimals: number): string {
  return new Intl.NumberFormat(language, {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  }).format(value);
}

/** "18.4 MB"; plain bytes below 1 KB. */
export function formatBytes(bytes: number, language: string): string {
  const { value, unit } = scale(bytes);
  return `${number(value, language, unit === "B" ? 0 : 1)} ${unit}`;
}

/** The parts of the progress line "62 % · 11.4 of 18.4 MB · 4.8 MB/s · about 2 s left". */
export interface ProgressParts {
  percent: number;
  /** In the unit of `size`, without the unit, as in "11.4 of 18.4 MB". */
  received: string;
  size: string;
  speed: string;
  /** `null` while the speed is not known yet. */
  left: string | null;
}

export function progressParts(
  received: number,
  size: number,
  bytesPerSec: number,
  language: string,
): ProgressParts {
  const total = scale(size);
  const decimals = total.unit === "B" ? 0 : 1;
  const done = Math.min(received, size);
  return {
    percent: size > 0 ? Math.floor((done / size) * 100) : 0,
    received: number(done / total.divisor, language, decimals),
    size: formatBytes(size, language),
    speed: `${formatBytes(bytesPerSec, language)}/s`,
    left: bytesPerSec > 0 ? formatDuration((size - done) / bytesPerSec, language) : null,
  };
}

/** "2 s", "3 min", "1 h": the rough time left, rounded up. */
export function formatDuration(seconds: number, language: string): string {
  const s = Math.max(1, Math.ceil(seconds));
  if (s < 60) return `${number(s, language, 0)} s`;
  if (s < 3600) return `${number(Math.ceil(s / 60), language, 0)} min`;
  return `${number(Math.ceil(s / 3600), language, 0)} h`;
}

/** The setting's value as shown in "Larger than your limit of {limit}". */
export function maxSizeLabel(limit: FilesMaxSize, language: string): string {
  const units: Record<FilesMaxSize, [number, string]> = {
    "100M": [100, "MB"],
    "500M": [500, "MB"],
    "1G": [1, "GB"],
    "2G": [2, "GB"],
  };
  const [value, unit] = units[limit];
  return `${number(value, language, 0)} ${unit}`;
}

/** Keys of `files.types.*`. */
export type FileTypeKey =
  | "image"
  | "video"
  | "audio"
  | "pdf"
  | "document"
  | "spreadsheet"
  | "presentation"
  | "archive"
  | "text"
  | "program"
  | "windowsProgram"
  | "script"
  | "other";

const BY_EXTENSION: Record<string, FileTypeKey> = {
  pdf: "pdf",
  doc: "document",
  docx: "document",
  odt: "document",
  rtf: "document",
  xls: "spreadsheet",
  xlsx: "spreadsheet",
  ods: "spreadsheet",
  csv: "spreadsheet",
  ppt: "presentation",
  pptx: "presentation",
  odp: "presentation",
  zip: "archive",
  "7z": "archive",
  rar: "archive",
  tar: "archive",
  gz: "archive",
  xz: "archive",
  bz2: "archive",
  zst: "archive",
  txt: "text",
  md: "text",
  log: "text",
  exe: "windowsProgram",
  msi: "windowsProgram",
  msix: "windowsProgram",
  msp: "windowsProgram",
  appx: "windowsProgram",
  scr: "windowsProgram",
  com: "windowsProgram",
  bat: "script",
  cmd: "script",
  js: "script",
  jse: "script",
  vbs: "script",
  vbe: "script",
  wsf: "script",
  ps1: "script",
  psm1: "script",
  sh: "script",
  command: "script",
  py: "script",
  pl: "script",
};

function extension(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
}

/**
 * The type label of a file. A risky file (the engine's FR-7.24 check, which also catches double
 * extensions) is always a program or script, whatever its MIME type claims.
 */
export function fileTypeKey(name: string, mime: string, risky: boolean): FileTypeKey {
  const byExtension = BY_EXTENSION[extension(name)];
  if (risky) {
    return byExtension === "windowsProgram" || byExtension === "script" ? byExtension : "program";
  }
  if (byExtension !== undefined) return byExtension;
  const kind = mime.split("/")[0]?.toLowerCase();
  if (kind === "image" || kind === "video" || kind === "audio") return kind;
  if (mime.toLowerCase() === "application/pdf") return "pdf";
  if (kind === "text") return "text";
  return "other";
}

/** "3f9a…c21e": the first and last four characters of the checksum. */
export function shortHash(sha256: string): string {
  return sha256.length > 8 ? `${sha256.slice(0, 4)}…${sha256.slice(-4)}` : sha256;
}

/** Which `files.when.*` text the share's end uses, with the local clock time. */
export function endsWhen(
  expires: number,
  now: Date,
  language: string,
): { key: "today" | "tomorrow"; time: string } {
  const end = new Date(expires * 1000);
  const time = new Intl.DateTimeFormat(language, { hour: "2-digit", minute: "2-digit" }).format(
    end,
  );
  const sameDay = end.toDateString() === now.toDateString();
  return { key: sameDay ? "today" : "tomorrow", time };
}

/** The folder part of a saved file's path, for "Saved to {folder}". */
export function folderOf(path: string): string {
  const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return cut > 0 ? path.slice(0, cut) : path;
}
