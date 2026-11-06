/** Pure helpers of the camera preview dialog (FR-8.10–8.14): errors, backgrounds, load line. */
import type { TFunction } from "i18next";
import {
  EngineError,
  type CameraBackground,
  type CameraDevice,
  type CameraEffects,
  type CameraFailure,
} from "../../../api";

const FAILURES: readonly CameraFailure[] = [
  "notConnected",
  "busy",
  "noCamera",
  "permissionDenied",
  "deviceGone",
  "unreadable",
  "cameraFailed",
  "roomFull",
  "relayFull",
  "useNotAllowed",
  "denied",
  "relayDown",
  "rateLimited",
  "mode",
  "noCertificate",
];

function isFailure(value: string): value is CameraFailure {
  return (FAILURES as readonly string[]).includes(value);
}

/**
 * The reason of a failed camera command: `notConnected` as its own code, every other reason as
 * code `camera` with the reason in the message (G-1). Anything else is `cameraFailed`.
 */
export function cameraFailureOf(error: unknown): CameraFailure {
  if (error instanceof EngineError) {
    if (error.code === "notConnected") return "notConnected";
    if (error.code === "camera" && isFailure(error.message)) return error.message;
  }
  return "cameraFailed";
}

/** The OS family from the backend's OS description ("windows x86_64"); `null` until known. */
export type DesktopOs = "windows" | "macos" | "linux";

export function desktopOs(os: string): DesktopOs | null {
  if (os.startsWith("windows")) return "windows";
  if (os.startsWith("macos")) return "macos";
  if (os.startsWith("linux")) return "linux";
  return null;
}

/** The text of a failure (section 5.6, FR-8.51); camera access follows the OS. */
export function failureText(
  t: TFunction<"camera">,
  failure: CameraFailure,
  os: DesktopOs | null,
): string {
  switch (failure) {
    case "notConnected":
      return t("notInChannel");
    case "permissionDenied":
      return t(`privacy.${os ?? "linux"}`);
    // The answers to Turn on camera (FR-8.51), worded for a start rather than a stop.
    case "useNotAllowed":
      return t("preview.errorUseNotAllowed");
    case "relayDown":
      return t("preview.errorRelayDown");
    default:
      return t(`errors.${failure}`);
  }
}

/** The "Camera stopped: …" notice of a camera that was on and stopped by itself (section 5.6). */
export function stoppedText(t: TFunction<"camera">, failure: CameraFailure): string {
  switch (failure) {
    case "relayDown":
      return t("errors.relayDown");
    case "relayEnded":
      return t("errors.endedByRelay");
    default:
      return failureText(t, failure, null);
  }
}

/** Only the Linux portal can simply be asked again; the others need the OS settings. */
export function canRetryAccess(failure: CameraFailure, os: DesktopOs | null): boolean {
  return failure === "permissionDenied" && (os === "linux" || os === null);
}

/** The 18 built-in images (FR-8.39), in display order; `key` names the text in `backgrounds`. */
export const BUILTIN_BACKGROUNDS = [
  { id: "abstract-warm-waves", key: "abstractWarmWaves" },
  { id: "blue-magenta-waves", key: "blueMagentaWaves" },
  { id: "glossy-waves-orb", key: "glossyWavesOrb" },
  { id: "pixel-mountain-sunset", key: "pixelMountainSunset" },
  { id: "city-balcony-night", key: "cityBalconyNight" },
  { id: "modern-wc", key: "modernWc" },
  { id: "electronics-lab", key: "electronicsLab" },
  { id: "server-corridor", key: "serverCorridor" },
  { id: "neon-gaming-room", key: "neonGamingRoom" },
  { id: "neon-nerd-room", key: "neonNerdRoom" },
  { id: "pixel-gaming-room-cat", key: "pixelGamingRoomCat" },
  { id: "reef-aquarium-room", key: "reefAquariumRoom" },
  { id: "night-model-railway", key: "nightModelRailway" },
  { id: "model-railway-viaduct", key: "modelRailwayViaduct" },
  { id: "spaceship-bridge-cat", key: "spaceshipBridgeCat" },
  { id: "space-station-earth", key: "spaceStationEarth" },
  { id: "scifi-room-golden-valley", key: "scifiRoomGoldenValley" },
  { id: "underwater-base-whale", key: "underwaterBaseWhale" },
] as const;

export type BackgroundMode = "none" | "blur" | "image";

export function backgroundMode(background: CameraBackground): BackgroundMode {
  return background.kind === "builtin" || background.kind === "own" ? "image" : background.kind;
}

export function sameBackground(a: CameraBackground, b: CameraBackground): boolean {
  if (a.kind !== b.kind) return false;
  return "id" in a && "id" in b ? a.id === b.id : true;
}

/**
 * The background a mode button picks: the last image when Image is chosen again, else the first
 * built-in image.
 */
export function backgroundForMode(
  mode: BackgroundMode,
  lastImage: CameraBackground | null,
): CameraBackground {
  if (mode === "none") return { kind: "none" };
  if (mode === "blur") return { kind: "blur" };
  return lastImage ?? { kind: "builtin", id: BUILTIN_BACKGROUNDS[0].id };
}

/** What the line under the background choice says (FR-8.33, FR-8.36, FR-8.37). */
export type LoadLine =
  | { kind: "unavailable"; reason: string }
  | { kind: "tooBusy" }
  | { kind: "load"; load: NonNullable<CameraEffects["load"]>; level: CameraEffects["level"] }
  | { kind: "idle" };

export function loadLine(effects: CameraEffects | null): LoadLine | null {
  if (effects === null) return null;
  if (effects.unavailable !== null) return { kind: "unavailable", reason: effects.unavailable };
  if (effects.tooBusy) return { kind: "tooBusy" };
  if (effects.load === null) return { kind: "idle" };
  return { kind: "load", load: effects.load, level: effects.level };
}

export function loadLineText(t: TFunction<"camera">, line: LoadLine): string {
  switch (line.kind) {
    case "unavailable":
      return t("preview.effectsUnavailable", { reason: line.reason });
    case "tooBusy":
      return t("effectsTooHeavy");
    case "idle":
      return t("preview.effectsIdle");
    case "load": {
      const load = t(`preview.load${capitalize(line.load)}`);
      return line.level === null
        ? t("preview.effectsLoadOnly", { load })
        : t("preview.effectsLoad", {
            load,
            unit: t(line.level === "best" ? "preview.levelBest" : "preview.levelLight"),
          });
    }
  }
}

function capitalize<T extends string>(value: T): Capitalize<T> {
  return (value.charAt(0).toUpperCase() + value.slice(1)) as Capitalize<T>;
}

/** The camera the picker shows first: the remembered one, else the system default. */
export function initialDevice(
  devices: readonly CameraDevice[],
  remembered: string | null,
): CameraDevice | undefined {
  return (
    devices.find((device) => device.id === remembered) ??
    devices.find((device) => device.isDefault) ??
    devices[0]
  );
}

/** Light is the engine's default blur strength (`DEFAULT_CAMERA_BLUR_STRENGTH`). */
export const BLUR_LIGHT = 50;
/** Strong is the engine's maximum blur strength. */
export const BLUR_STRONG = 100;

/** Older or hand-edited settings may hold any 0–100 value; above Light counts as Strong. */
export function isStrongBlur(strength: number): boolean {
  return strength > BLUR_LIGHT;
}
