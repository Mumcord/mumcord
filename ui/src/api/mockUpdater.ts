/**
 * Updater of the mock engine: "Check now" finds a fictional release with three versions of
 * notes, and a download runs through every phase with simulated progress, so the update UI can
 * be tried and photographed without GitHub. With `startupCheck` a check runs right after the
 * start, like the scheduled check of the real app.
 */
import type { Settings, UpdateOffer, UpdateStatus } from "../bindings/commands";

const OFFER: UpdateOffer = {
  version: "0.4.5",
  url: "https://github.com/Mumcord/mumcord/releases/tag/v0.4.5",
  notes: [
    {
      version: "0.4.5",
      date: "2026-11-12",
      changes: [
        "### Added",
        "- Pick which input device a call starts with.",
        "### Changed",
        "- Calmer reconnect banner.",
        "### Fixed",
        "- Rare echo after switching headsets.",
      ].join("\n"),
    },
    {
      version: "0.4.4",
      date: "2026-11-03",
      changes: [
        "### Added",
        "- Keyboard shortcut to jump to the **talking** person.",
        "### Fixed",
        "- The channel list kept an old name after a rename.",
      ].join("\n"),
    },
    {
      version: "0.4.3",
      date: "2026-10-24",
      changes: ["### Fixed", "- Push to talk stuck after `Alt+Tab` on Windows."].join("\n"),
    },
  ],
};

const BUNDLE_BYTES = 94_000_000;
const TICK_MS = 200;
const TICKS = 25;
const CHECK_MS = 600;
const STEP_MS = 700;
/** Leaves time to connect to a demo server first, as the real check runs after the start. */
const STARTUP_CHECK_MS = 3000;

export interface MockUpdater {
  status: () => UpdateStatus;
  checkNow: () => UpdateStatus;
  download: () => void;
  cancel: () => void;
  restart: () => void;
}

export function createMockUpdater(options: {
  demo: boolean;
  startupCheck: boolean;
  settings: () => Settings;
  emit: (status: UpdateStatus, prompt: boolean) => void;
}): MockUpdater {
  let status: UpdateStatus = {
    currentVersion: options.demo ? "0.4.2" : "0.0.0-mock",
    install: "appImage",
    canInstall: true,
    devBuild: false,
    phase: "idle",
    offer: null,
    progress: null,
    error: null,
    lastCheckMs: null,
  };
  let timer: ReturnType<typeof setTimeout> | null = null;

  const set = (change: Partial<UpdateStatus>, prompt = false) => {
    status = { ...status, ...change };
    options.emit({ ...status }, prompt);
  };

  const stop = () => {
    if (timer !== null) clearTimeout(timer);
    timer = null;
  };

  const later = (ms: number, fn: () => void) => {
    timer = setTimeout(fn, ms);
  };

  const download = () => {
    stop();
    let tick = 0;
    set({ phase: "downloading", error: null, progress: { received: 0, total: BUNDLE_BYTES } });
    const step = () => {
      tick += 1;
      if (tick < TICKS) {
        set({ progress: { received: (BUNDLE_BYTES * tick) / TICKS, total: BUNDLE_BYTES } });
        later(TICK_MS, step);
        return;
      }
      set({ phase: "verifying", progress: { received: BUNDLE_BYTES, total: BUNDLE_BYTES } });
      later(STEP_MS, () => {
        set({ phase: "installing" });
        later(STEP_MS, () => {
          timer = null;
          set({ phase: "ready" }, true);
        });
      });
    };
    later(TICK_MS, step);
  };

  const checkNow = () => {
    const settings = options.settings();
    if (status.devBuild || settings.updateCheck !== "yes") return { ...status };
    stop();
    set({ phase: "checking" });
    later(CHECK_MS, () => {
      timer = null;
      const current = options.settings();
      const skipped = current.updateSkipped === OFFER.version;
      const fresh = !skipped && status.offer === null;
      set(
        {
          phase: skipped ? "upToDate" : "available",
          offer: skipped ? null : OFFER,
          lastCheckMs: Date.now(),
        },
        fresh && !current.updateAutoDownload,
      );
      if (fresh && current.updateAutoDownload) download();
    });
    return { ...status };
  };

  if (options.startupCheck) {
    later(STARTUP_CHECK_MS, () => {
      checkNow();
    });
  }

  return {
    status: () => ({ ...status }),
    checkNow,
    download,
    cancel: () => {
      stop();
      set({ phase: "available", progress: null });
    },
    restart: () => {
      // The mock cannot restart; it acts as if the new version came back up.
      stop();
      set({
        currentVersion: status.offer?.version ?? status.currentVersion,
        phase: "upToDate",
        offer: null,
        progress: null,
      });
    },
  };
}
