import { useCallback, useEffect, useMemo, useState } from "react";
import { getApi, type Settings } from "../../api";
import type { PresetDraft } from "../wizard/presets";
import { baselineDraft, changedSettings, defaultDraft, type TrackedSetting } from "./changes";
import type { UpdateSettings } from "./useSettings";

/** The in-game overlay as a preset value: on/off, or `null` where it does not exist. */
type Overlay = boolean | null;

export interface SettingsChanges {
  /** The current values including overlay and Do Not Disturb; `null` while loading. */
  current: PresetDraft | null;
  /** What a fresh install starts with, for this platform; `null` while loading. */
  defaults: PresetDraft | null;
  /** What every setting is compared with (defaults plus the active preset). */
  baseline: PresetDraft | null;
  /** The settings that differ from the baseline. */
  changed: TrackedSetting[];
  /** True when the overlay or Do Not Disturb part of the last `apply` could not be saved. */
  applyFailed: boolean;
  /** Saves a whole draft: the settings record plus overlay and Do Not Disturb where they differ. */
  apply: (next: PresetDraft) => Promise<void>;
}

/**
 * Loads the default settings and the two preset values that live behind their own commands, and
 * keeps the overlay and Do Not Disturb values current while other cards change them.
 */
export function useSettingsChanges(
  settings: Settings | null,
  update: UpdateSettings,
): SettingsChanges {
  const [defaultSettings, setDefaultSettings] = useState<Settings | null>(null);
  const [overlay, setOverlay] = useState<Overlay | undefined>(undefined);
  const [dndFollow, setDndFollow] = useState<boolean | null>(null);
  const [applyFailed, setApplyFailed] = useState(false);

  useEffect(() => {
    let active = true;
    const api = getApi();
    const offDnd = api.on("dndChanged", ({ state }) => {
      setDndFollow(state.followSystem);
    });
    const offOverlay = api.on("inGameOverlayChanged", ({ state }) => {
      setOverlay(state.supported ? state.mode === "on" : null);
    });
    api.settingsDefault().then(
      (loaded) => {
        if (active) setDefaultSettings(loaded);
      },
      (error: unknown) => {
        console.error("Loading the default settings failed", error);
      },
    );
    api.ingameOverlayState().then(
      (state) => {
        if (active) setOverlay(state.supported ? state.mode === "on" : null);
      },
      (error: unknown) => {
        // Without the overlay state the overlay is left out, like on a system without one.
        console.error("Reading the in-game overlay state failed", error);
        if (active) setOverlay(null);
      },
    );
    api.dndState().then(
      (state) => {
        if (active) setDndFollow(state.followSystem);
      },
      (error: unknown) => {
        console.error("Reading the Do Not Disturb state failed", error);
      },
    );
    return () => {
      active = false;
      offDnd();
      offOverlay();
    };
  }, []);

  const current = useMemo<PresetDraft | null>(
    () =>
      settings === null || overlay === undefined || dndFollow === null
        ? null
        : { settings, ingameOverlay: overlay, dndFollowSystem: dndFollow },
    [settings, overlay, dndFollow],
  );
  const defaults = useMemo(
    () =>
      defaultSettings === null || overlay === undefined
        ? null
        : defaultDraft(defaultSettings, overlay !== null),
    [defaultSettings, overlay],
  );
  const baseline = useMemo(
    () =>
      defaults === null || settings === null ? null : baselineDraft(defaults, settings.setupPreset),
    [defaults, settings],
  );
  const changed = useMemo(
    () => (current === null || baseline === null ? [] : changedSettings(current, baseline)),
    [current, baseline],
  );

  const apply = useCallback(
    async (next: PresetDraft) => {
      update(next.settings);
      const api = getApi();
      try {
        if (next.ingameOverlay !== null && next.ingameOverlay !== overlay) {
          const state = await api.ingameOverlaySetMode(next.ingameOverlay ? "on" : "off");
          setOverlay(state.supported ? state.mode === "on" : null);
        }
        if (next.dndFollowSystem !== dndFollow) {
          const state = await api.dndSetFollowSystem(next.dndFollowSystem);
          setDndFollow(state.followSystem);
        }
        setApplyFailed(false);
      } catch (error: unknown) {
        console.error("Saving the in-game overlay or Do Not Disturb failed", error);
        setApplyFailed(true);
      }
    },
    [update, overlay, dndFollow],
  );

  return { current, defaults, baseline, changed, applyFailed, apply };
}
