import { useCallback, useEffect, useRef, useState } from "react";
import { getApi, type Settings } from "../../api";
import { usePreferences } from "../../app/preferences";

export type UpdateSettings = (patch: Partial<Settings>) => void;

export interface SettingsState {
  /** Settings as shown, including changes that are still being saved; `null` while loading. */
  settings: Settings | null;
  loadFailed: boolean;
  saveFailed: boolean;
  /** Saves a change immediately (no Save button), one request at a time. */
  update: UpdateSettings;
}

/**
 * Loads the settings record and saves every change right away. Saves run one after another so a
 * later change can never be overwritten by an earlier, slower one; a failed save rolls the
 * screen back to the last record the engine confirmed.
 */
export function useSettings(revision = 0): SettingsState {
  const { applySettings } = usePreferences();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const [saveFailed, setSaveFailed] = useState(false);
  const confirmed = useRef<Settings | null>(null);
  const desired = useRef<Settings | null>(null);
  const pending = useRef(0);
  const queue = useRef<Promise<void>>(Promise.resolve());

  useEffect(() => {
    let active = true;
    // Unsaved edits win: a reload must never overwrite what the user is changing right now.
    if (pending.current > 0) return;
    getApi()
      .settingsGet()
      .then(
        (loaded) => {
          if (!active) return;
          confirmed.current = loaded;
          desired.current = loaded;
          setSettings(loaded);
        },
        (error: unknown) => {
          console.error("Loading settings failed", error);
          if (active) setLoadFailed(true);
        },
      );
    return () => {
      active = false;
    };
  }, [revision]);

  const update = useCallback<UpdateSettings>(
    (patch) => {
      if (!desired.current) return;
      desired.current = { ...desired.current, ...patch };
      setSettings(desired.current);
      pending.current += 1;
      // Each save applies its change to the latest record the engine confirmed.
      queue.current = queue.current.then(async () => {
        try {
          const base = confirmed.current;
          if (!base) return;
          const saved = await getApi().settingsUpdate({ ...base, ...patch });
          confirmed.current = saved;
          applySettings(saved);
          setSaveFailed(false);
          if (pending.current === 1) {
            desired.current = saved;
            setSettings(saved);
          }
        } catch (error: unknown) {
          console.error("Saving settings failed", error);
          desired.current = confirmed.current;
          setSettings(confirmed.current);
          setSaveFailed(true);
        } finally {
          pending.current -= 1;
        }
      });
    },
    [applySettings],
  );

  return { settings, loadFailed, saveFailed, update };
}
