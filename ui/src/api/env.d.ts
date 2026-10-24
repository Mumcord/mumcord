interface ImportMetaEnv {
  /** "1" forces the in-memory mock engine (see ui/src/api/index.ts). */
  readonly VITE_MUMCORD_MOCK?: string;
  /** Mock seed when the mock runs (see MockOptions in ui/src/api/mock.ts). */
  readonly VITE_MUMCORD_MOCK_SEED?: "fresh" | "ready" | "demo" | "demoFresh";
}
