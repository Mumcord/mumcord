/**
 * The polite live-region messages of FR-8.69: "{name} turned on their camera" and "… turned off
 * their camera". Changes within 2 s are bundled into one message per direction ("Jonas and Maya
 * turned on their cameras"), so a busy room does not flood the screen reader.
 */

/** Changes within this window are spoken together. */
export const BUNDLE_MS = 2000;

export interface CameraChange {
  name: string;
  on: boolean;
}

export interface Bundle {
  on: boolean;
  names: string[];
}

/**
 * Groups the changes of one window into at most two bundles, turned on first. A person who turned
 * on and off again within the window is spoken only by their last change.
 */
export function bundleChanges(changes: readonly CameraChange[]): Bundle[] {
  const last = new Map<string, boolean>();
  for (const change of changes) {
    last.delete(change.name);
    last.set(change.name, change.on);
  }
  const bundles: Bundle[] = [];
  for (const on of [true, false]) {
    const names = [...last].filter(([, value]) => value === on).map(([name]) => name);
    if (names.length > 0) bundles.push({ on, names });
  }
  return bundles;
}

export interface Announcer {
  push(change: CameraChange): void;
  /** Drops what is still waiting, e.g. when the room unmounts. */
  cancel(): void;
}

/** Collects changes and hands each window's bundles to `speak` once the window ends. */
export function createAnnouncer(speak: (bundles: Bundle[]) => void): Announcer {
  let pending: CameraChange[] = [];
  let timer: ReturnType<typeof setTimeout> | null = null;
  return {
    push(change) {
      pending.push(change);
      if (timer !== null) return;
      timer = setTimeout(() => {
        timer = null;
        const bundles = bundleChanges(pending);
        pending = [];
        if (bundles.length > 0) speak(bundles);
      }, BUNDLE_MS);
    },
    cancel() {
      if (timer !== null) clearTimeout(timer);
      timer = null;
      pending = [];
    },
  };
}
