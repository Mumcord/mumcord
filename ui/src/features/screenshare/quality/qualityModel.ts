/**
 * What the sharer's live bar shows about how the own share is sent (FR-7.38, FR-7.39) and what
 * its Quality menu offers (mockup 02), as pure logic the components and tests share.
 */
import type {
  ActiveEncoder,
  QualityChoice,
  ScreenFps,
  ScreenOptimize,
  ScreenResolution,
  ViewerQuality,
} from "../../../api";

/** One row of the Quality menu: a resolution and frame rate ceiling and what it needs. */
export interface QualityPreset {
  resolution: Exclude<ScreenResolution, "auto">;
  fps: ScreenFps;
  /** The upload the step needs, as the menu names it (mockup 02). */
  mbit: number;
}

/** The menu's fixed steps, best first; "Auto" sits above them. 4K stays hidden (user decision). */
export const QUALITY_PRESETS: readonly QualityPreset[] = [
  { resolution: "1440", fps: "60", mbit: 9 },
  { resolution: "1080", fps: "60", mbit: 6 },
  { resolution: "1080", fps: "30", mbit: 4 },
  { resolution: "720", fps: "30", mbit: 2 },
];

/** "Optimize for" rows; the menu leaves out Balanced, as mockup 02 does. */
export const OPTIMIZE_CHOICES: readonly Exclude<ScreenOptimize, "balanced">[] = ["text", "motion"];

/** A menu row: "auto" or the index of a preset. */
export type QualityRow = "auto" | number;

/** The row that matches the current pick; `null` when the pick is none of the rows. */
export function checkedRow(choice: QualityChoice): QualityRow | null {
  if (choice.resolution === "auto") return "auto";
  const at = QUALITY_PRESETS.findIndex(
    (preset) => preset.resolution === choice.resolution && preset.fps === choice.fps,
  );
  return at < 0 ? null : at;
}

/**
 * The pick after choosing a row. "Auto" keeps the frame rate from Settings so the engine picks
 * the steps within it; Optimize for and the upload limit stay as they are.
 */
export function chooseRow(
  choice: QualityChoice,
  row: QualityRow,
  settingsFps: ScreenFps,
): QualityChoice {
  if (row === "auto") return { ...choice, resolution: "auto", fps: settingsFps };
  const preset = QUALITY_PRESETS[row];
  if (preset === undefined) return choice;
  return { ...choice, resolution: preset.resolution, fps: preset.fps };
}

/** The codec badge: "AV1 · hardware", "AV1 · CPU", "VP9 · hardware" or "VP9 · CPU". */
export interface CodecBadge {
  codec: "AV1" | "VP9";
  hardware: boolean;
}

export function codecBadge(encoder: ActiveEncoder | null): CodecBadge | null {
  if (encoder === null) return null;
  return { codec: codecName(encoder.codec), hardware: encoder.gpu !== null };
}

export function codecName(codec: ActiveEncoder["codec"]): "AV1" | "VP9" {
  return codec === "av1" ? "AV1" : "VP9";
}

/**
 * The path badge: "relay" as soon as the relay carries the share to anyone, "direct" while every
 * viewer gets a direct copy, and nothing without viewers.
 */
export function sharePath(viewers: readonly ViewerQuality[]): "relay" | "direct" | null {
  if (viewers.length === 0) return null;
  return viewers.some((viewer) => viewer.path !== "direct") ? "relay" : "direct";
}
