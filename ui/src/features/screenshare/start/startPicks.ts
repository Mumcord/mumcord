/** The picks of the share start dialog (FR-7.35) and the texts derived from them. */
import type { TFunction } from "i18next";
import type {
  ActiveEncoder,
  QualityChoice,
  ScreenCodec,
  ScreenFps,
  ScreenOptimize,
  ScreenResolution,
  ScreenUploadLimit,
  Settings,
  SharePreview,
} from "../../../api";

export interface StartPicks {
  choice: QualityChoice;
  codec: ScreenCodec;
  sound: boolean;
}

// "Source" (4K) stays hidden until it is built (user decision).
export const RESOLUTIONS: readonly ScreenResolution[] = ["auto", "720", "1080", "1440"];
export const FPS: readonly ScreenFps[] = ["15", "30", "60"];
export const OPTIMIZE: readonly ScreenOptimize[] = ["balanced", "text", "motion"];
export const CODECS: readonly ScreenCodec[] = ["auto", "av1", "vp9"];
export const UPLOAD_LIMITS: readonly ScreenUploadLimit[] = ["auto", "2", "5", "10", "20"];

/** The dialog opens with the defaults of Settings → Screen sharing. */
export function picksFromSettings(settings: Settings): StartPicks {
  return {
    choice: {
      resolution: settings.screenResolution,
      fps: settings.screenFps,
      optimize: settings.screenOptimize,
      uploadLimit: settings.screenUploadLimit,
    },
    codec: settings.screenCodec,
    sound: settings.screenSound,
  };
}

/** "Use these settings next time": the picks become the defaults of Settings → Screen sharing. */
export function settingsWithPicks(settings: Settings, picks: StartPicks): Settings {
  return {
    ...settings,
    screenResolution: picks.choice.resolution,
    screenFps: picks.choice.fps,
    screenOptimize: picks.choice.optimize,
    screenUploadLimit: picks.choice.uploadLimit,
    screenCodec: picks.codec,
    screenSound: picks.sound,
  };
}

export function resolutionLabel(t: TFunction<"screenshare">, resolution: ScreenResolution) {
  return resolution === "auto"
    ? t("start.resolutionAuto")
    : t(`start.resolution${resolution}` as const);
}

/** "AV1 (graphics card)", "AV1 (software)", "VP9 (graphics card)" or "VP9". */
export function encoderLabel(t: TFunction<"screenshare">, encoder: ActiveEncoder) {
  if (encoder.codec === "av1") {
    return encoder.gpu === null ? t("start.encoderAv1Cpu") : t("start.encoderAv1Gpu");
  }
  return encoder.gpu === null ? t("start.encoderVp9Cpu") : t("start.encoderVp9Gpu");
}

/** The label of a codec option; it names the encoder that option would use once that is known. */
export function codecOptionLabel(
  t: TFunction<"screenshare">,
  codec: ScreenCodec,
  preview: SharePreview | undefined,
) {
  if (codec === "auto") {
    return preview === undefined
      ? t("start.codecAutoPending")
      : t("start.codecAuto", { encoder: encoderLabel(t, preview.encoder) });
  }
  if (preview === undefined) return codec === "av1" ? t("start.codecAv1") : t("start.codecVp9");
  return encoderLabel(t, preview.encoder);
}

/** "up to 1080p · 30 fps · AV1 · about 4 Mbit/s" (FR-7.36). */
export function viewersGetSummary(t: TFunction<"screenshare">, preview: SharePreview) {
  return t("start.viewersGetSummary", {
    resolution: `${String(preview.height)}p`,
    fps: preview.fps,
    codec: preview.encoder.codec === "av1" ? t("start.codecAv1") : t("start.codecVp9"),
    mbit: mbit(preview.kbps),
  });
}

/** Whole numbers stay whole ("4"), the small steps keep one decimal ("1.2", "0.7"). */
function mbit(kbps: number) {
  const value = Math.round(kbps / 100) / 10;
  return Number.isInteger(value) ? String(value) : value.toFixed(1);
}

/** The note under the codec choice, or `null` when everything runs as picked. */
export function codecNote(
  t: TFunction<"screenshare">,
  picks: StartPicks,
  preview: SharePreview,
): string | null {
  if (preview.softwareWarning) return t("start.softwareWarning");
  switch (preview.av1Skipped) {
    case "aboveSoftwareLimit":
      return t("start.av1SoftwareLimit");
    case "tooHeavy":
      return t("start.av1SkippedTooHeavy");
    case "onBattery":
      return t("start.av1SkippedOnBattery");
    case null:
      break;
  }
  // A manual AV1 pick in software is capped at 1080p30 (FR-7.37) instead of switching codec.
  const above = picks.choice.resolution === "1440" || picks.choice.fps === "60";
  if (picks.codec === "av1" && preview.encoder.codec === "av1" && preview.encoder.gpu === null) {
    return above ? t("start.av1SoftwareLimit") : null;
  }
  return null;
}
