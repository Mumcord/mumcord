import type { Settings, SetupPreset } from "../../api";
import { updateModePatch, type UpdateMode } from "../updates/updateMode";
import {
  applyPreset,
  matchingPreset,
  PRESET_KEYS,
  presetValueOf,
  type PresetDraft,
  type PresetId,
} from "./presets";

/**
 * Everything the setup wizard edits (issue #159). Nothing reaches the engine before Finish: the
 * wizard keeps this draft and, at Finish, writes only what differs from the stored settings.
 */
export interface WizardDraft extends PresetDraft {
  /** Register Mumcord as the `mumble://` handler at Finish (FR-1.6); an action, not a value. */
  registerLinkHandler: boolean;
}

/**
 * The draft with `preset` applied. Maximum privacy also sets updates to "Notify only" (Q1.12);
 * the other two presets keep the update mode the wizard started with.
 */
export function applyStyle(
  draft: WizardDraft,
  preset: PresetId,
  initialUpdateMode: UpdateMode,
): WizardDraft {
  const applied = applyPreset(draft, preset);
  const updateMode = preset === "maximumPrivacy" ? "notify" : initialUpdateMode;
  return {
    ...draft,
    ...applied,
    settings: { ...applied.settings, ...updateModePatch(updateMode) },
  };
}

/** Whether any value a preset covers differs between the two drafts. */
export function presetValuesChanged(before: PresetDraft, after: PresetDraft): boolean {
  return PRESET_KEYS.some((key) => presetValueOf(before, key) !== presetValueOf(after, key));
}

/**
 * The preset to record at Finish: the chosen preset card, or for Custom the preset the values
 * match exactly, else "custom".
 */
export function recordedPreset(style: SetupPreset, draft: PresetDraft): SetupPreset {
  return style === "custom" ? (matchingPreset(draft) ?? "custom") : style;
}

/**
 * The top-level settings whose value differs between `before` and `after`; nested records
 * (private messages, sounds, …) count as one value and are compared by content.
 */
export function changedSettings(before: Settings, after: Settings): Partial<Settings> {
  const patch: Partial<Settings> = {};
  for (const key of Object.keys(after)) {
    if (!isSettingsKey(after, key)) continue;
    if (JSON.stringify(before[key]) !== JSON.stringify(after[key])) {
      Object.assign(patch, { [key]: after[key] });
    }
  }
  return patch;
}

function isSettingsKey(settings: Settings, key: string): key is keyof Settings {
  return key in settings;
}
