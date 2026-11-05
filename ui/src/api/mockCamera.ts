/**
 * The browser mock's camera (spec 08): two cameras, a preview that draws a moving test picture,
 * Turn on after a short delay and a background library with one own image.
 */
import type { Channel } from "@tauri-apps/api/core";
import type {
  CameraBackground,
  CameraBackgroundImage,
  CameraDevice,
  CameraEffects,
  CameraState,
  Settings,
} from "../bindings/commands";
import { EngineError, type EngineCommands, type EventName, type EventPayloads } from "./client";

const DEVICES: CameraDevice[] = [
  { id: "mock-integrated", name: "Integrated Webcam (720p)", isDefault: true },
  { id: "mock-c920", name: "USB Camera C920", isDefault: false },
];

/** The built-in images, sorted by id like the engine's list. */
const BUILTIN: readonly [string, [string, string]][] = [
  ["beach", ["#9fd6f2", "#f1d9a6"]],
  ["library", ["#4a3424", "#a33"]],
  ["mountains", ["#7fb2e5", "#4f7a5a"]],
  ["night-city", ["#141a3a", "#313b70"]],
  ["soft-light", ["#8f7cff", "#ffc59e"]],
  ["studio", ["#ece6dc", "#c9b79c"]],
];

const FRAME_WIDTH = 640;
const FRAME_HEIGHT = 360;
const FRAME_MS = 100;
/** Header of a screen-share channel message (`ui/src/api/screenFrames.ts`), RGBA kind. */
const HEADER_LEN = 16;
const KIND_RGBA = 2;
const CODEC_VP9 = 1;
const TURN_ON_MS = 800;

/** An SVG thumbnail as a data URL, like the engine's `data:image/jpeg;base64,…`. */
function thumbnail([top, bottom]: [string, string]): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 160 90"><rect width="160" height="90" fill="${top}"/><rect y="60" width="160" height="30" fill="${bottom}"/></svg>`;
  return `data:image/svg+xml;base64,${btoa(svg)}`;
}

type CameraCommands = Pick<
  EngineCommands,
  | "cameraDevices"
  | "cameraView"
  | "cameraPreviewStart"
  | "cameraTurnOn"
  | "cameraTurnOff"
  | "cameraLocalPreviewStart"
  | "cameraLocalPreviewStop"
  | "cameraSetDevice"
  | "cameraSetBackground"
  | "cameraSetEffectQuality"
  | "cameraEffects"
  | "cameraPreviewAttach"
  | "cameraPreviewDetach"
  | "cameraBackgrounds"
  | "cameraOwnBackgroundAdd"
  | "cameraOwnBackgroundRemove"
  | "cameraDevicesWatchStart"
  | "cameraDevicesWatchStop"
>;

export function createMockCamera(
  emit: <K extends EventName>(event: K, payload: EventPayloads[K]) => void,
  getSettings: () => Settings,
  setSettings: (next: Settings) => void,
): CameraCommands {
  let serverId: number | null = null;
  let state: CameraState = "off";
  let background: CameraBackground = { kind: "blur" };
  let local = false;
  let frames: { channel: Channel<ArrayBuffer>; timer: number } | null = null;
  let own: CameraBackgroundImage[] = [
    { id: "mock-own-1", thumbnail: thumbnail(["#2a4d3c", "#1f3a2d"]) },
  ];
  let ownCount = own.length;

  const changed = () => {
    emit("cameraChanged", {
      view: { serverId: serverId ?? 0, state, error: null, broadcast: null, viewers: [] },
    });
  };

  const running = () => state !== "off" || local;

  const sendFrame = (channel: Channel<ArrayBuffer>, tick: number) => {
    if (!running()) return;
    const buffer = new ArrayBuffer(HEADER_LEN + FRAME_WIDTH * FRAME_HEIGHT * 4);
    const view = new DataView(buffer);
    view.setUint8(0, KIND_RGBA);
    view.setUint8(3, CODEC_VP9);
    view.setUint16(12, FRAME_WIDTH, true);
    view.setUint16(14, FRAME_HEIGHT, true);
    const pixels = new Uint8Array(buffer, HEADER_LEN);
    const headX = FRAME_WIDTH / 2 + Math.sin(tick / 10) * 12;
    const plain = background.kind === "none";
    for (let y = 0; y < FRAME_HEIGHT; y++) {
      for (let x = 0; x < FRAME_WIDTH; x++) {
        const i = (y * FRAME_WIDTH + x) * 4;
        const dx = x - headX;
        const head = dx * dx + (y - 140) ** 2 < 60 * 60;
        const body = y > 220 && Math.abs(dx) < 150 - (FRAME_HEIGHT - y) * 0.4;
        if (head) pixels.set([183, 121, 79, 255], i);
        else if (body) pixels.set([124, 92, 255, 255], i);
        else if (plain) pixels.set([(x * 7) & 255, 200, (y * 5) & 255, 255], i);
        else pixels.set([170 + ((x >> 5) & 15), 190, 160 + ((y >> 5) & 15), 255], i);
      }
    }
    channel.onmessage(buffer);
  };

  const stopFrames = () => {
    if (frames !== null) window.clearInterval(frames.timer);
    frames = null;
  };

  return {
    cameraDevices: () => Promise.resolve(DEVICES.map((device) => ({ ...device }))),
    cameraView: () =>
      Promise.resolve(
        state === "off" || serverId === null
          ? null
          : { serverId, state, error: null, broadcast: null, viewers: [] },
      ),
    cameraPreviewStart: (id) => {
      if (state !== "off") return Promise.reject(new EngineError("camera", "busy"));
      serverId = id;
      local = false;
      state = "preview";
      background = getSettings().cameraDefaultBackground;
      changed();
      return Promise.resolve(null);
    },
    cameraTurnOn: (id) => {
      if (state !== "preview" || id !== serverId) {
        return Promise.reject(new EngineError("camera", "busy"));
      }
      state = "requesting";
      changed();
      return new Promise((resolve) => {
        window.setTimeout(() => {
          state = "onAir";
          changed();
          resolve(null);
        }, TURN_ON_MS);
      });
    },
    cameraTurnOff: () => {
      if (state !== "off") {
        state = "off";
        changed();
      }
      stopFrames();
      return Promise.resolve();
    },
    cameraLocalPreviewStart: () => {
      if (state !== "off") return Promise.reject(new EngineError("camera", "busy"));
      local = true;
      return Promise.resolve(null);
    },
    cameraLocalPreviewStop: () => {
      local = false;
      return Promise.resolve();
    },
    cameraSetDevice: (deviceId) =>
      DEVICES.some((device) => device.id === deviceId)
        ? Promise.resolve(null)
        : Promise.reject(new EngineError("camera", "deviceGone")),
    cameraSetBackground: (next) => {
      background = next;
      return Promise.resolve(null);
    },
    cameraSetEffectQuality: () => Promise.resolve(),
    cameraEffects: (): Promise<CameraEffects> =>
      Promise.resolve(
        running() && background.kind !== "none"
          ? { unavailable: null, level: "best", load: "low", tooBusy: false }
          : { unavailable: null, level: null, load: running() ? "low" : null, tooBusy: false },
      ),
    cameraPreviewAttach: (channel) => {
      stopFrames();
      let tick = 0;
      const timer = window.setInterval(() => {
        tick += 1;
        sendFrame(channel, tick);
      }, FRAME_MS);
      frames = { channel, timer };
      sendFrame(channel, tick);
      return Promise.resolve();
    },
    cameraPreviewDetach: (channelId) => {
      if (frames?.channel.id === channelId) stopFrames();
      return Promise.resolve();
    },
    cameraBackgrounds: () =>
      Promise.resolve({
        builtin: BUILTIN.map(([id, colours]) => ({ id, thumbnail: thumbnail(colours) })),
        own: own.map((image) => ({ ...image })),
      }),
    cameraOwnBackgroundAdd: (path) => {
      if (!/\.(png|jpe?g|webp)$/i.test(path)) {
        return Promise.reject(new EngineError("invalidInput", "unsupported image"));
      }
      ownCount += 1;
      const image = {
        id: `mock-own-${String(ownCount)}`,
        thumbnail: thumbnail(["#5a4a7a", "#2d2540"]),
      };
      own = [...own, image];
      return Promise.resolve({ ...image });
    },
    cameraOwnBackgroundRemove: (id) => {
      if (!own.some((image) => image.id === id)) {
        return Promise.reject(new EngineError("notFound", "unknown image"));
      }
      own = own.filter((image) => image.id !== id);
      const settings = getSettings();
      const fallback = settings.cameraDefaultBackground;
      if (fallback.kind === "own" && fallback.id === id) {
        setSettings({ ...settings, cameraDefaultBackground: { kind: "blur" } });
      }
      return Promise.resolve(null);
    },
    cameraDevicesWatchStart: () => Promise.resolve(),
    cameraDevicesWatchStop: () => Promise.resolve(),
  };
}
