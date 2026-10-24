/**
 * Entry point for all engine access from the UI. Features call `getApi()` and never import
 * the generated bindings directly, so they run unchanged against the mock.
 */
import type { MumcordApi } from "./client";
import { createMockApi } from "./mock";
import { createTauriApi } from "./tauri";

export * from "./client";
export type * from "../bindings/commands";
export { createMockApi, type MockApi, type MockControls, type MockOptions } from "./mock";

export interface ApiEnvironment {
  /** True in `vite dev` and vitest. */
  dev: boolean;
  /** Vite mode; "test" under vitest. */
  mode: string;
  /** `VITE_MUMCORD_MOCK`; "1" forces the mock in any build. */
  forceMock: string | undefined;
  /** Whether the page runs inside the Tauri webview. */
  inTauri: boolean;
}

/**
 * Picks the implementation. Outside Tauri the mock is only used in dev/test builds or when
 * forced: a production build must never silently run without the engine.
 */
export function selectApiKind(env: ApiEnvironment): "tauri" | "mock" {
  if (env.forceMock === "1") return "mock";
  if (env.inTauri) return "tauri";
  if (env.dev || env.mode === "test") return "mock";
  throw new Error(
    "Mumcord engine unavailable: this build must run inside the desktop app (set VITE_MUMCORD_MOCK=1 to use the mock).",
  );
}

function currentEnvironment(): ApiEnvironment {
  return {
    dev: import.meta.env.DEV,
    mode: import.meta.env.MODE,
    forceMock: import.meta.env.VITE_MUMCORD_MOCK,
    inTauri: typeof window !== "undefined" && "__TAURI_INTERNALS__" in window,
  };
}

let instance: MumcordApi | undefined;

/** The active engine client, created on first use. */
export function getApi(): MumcordApi {
  instance ??= selectApiKind(currentEnvironment()) === "mock" ? createMockApi() : createTauriApi();
  return instance;
}

/** Replaces the active client (tests, Storybook-style previews); `undefined` resets it. */
export function setApi(api: MumcordApi | undefined): void {
  instance = api;
}
