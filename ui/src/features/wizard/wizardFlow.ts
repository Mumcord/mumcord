import type { SetupPreset } from "../../api";
import { RECOMMENDED_PRESET } from "./presets";

/**
 * Step order of the setup wizard (issue #159) as a pure state machine; the step components only
 * render what it says.
 *
 * First start: Welcome → [Mumble import (#66), when a Mumble profile was found] → Identity →
 * Voice → Setup style → [Custom only: private messages → connections → features → appearance →
 * startup] → summary. Rerun (#149): the same without import and identity, and the welcome step is
 * optional. Presets jump from the setup style straight to the summary.
 */

export type WizardStepId =
  | "welcome"
  | "import"
  | "identity"
  | "voice"
  | "setupStyle"
  | "privateMessages"
  | "connections"
  | "features"
  | "appearance"
  | "startup"
  | "summary";

/** The detailed steps only Custom walks through; the rail tags them "Custom". */
export const CUSTOM_STEPS: readonly WizardStepId[] = [
  "privateMessages",
  "connections",
  "features",
  "appearance",
  "startup",
];

/** Steps with a Skip button: skipping keeps their current values. */
const SKIPPABLE_STEPS: readonly WizardStepId[] = ["features", "appearance"];

export interface WizardFlowOptions {
  mode: "firstStart" | "rerun";
  /** A Mumble profile was found, so the import step (#66) is shown (first start only). */
  mumbleFound?: boolean;
  /** Rerun only: show the welcome step (default true). The first start always shows it. */
  includeWelcome?: boolean;
}

export interface WizardFlowState {
  readonly mode: "firstStart" | "rerun";
  readonly mumbleFound: boolean;
  readonly includeWelcome: boolean;
  readonly step: WizardStepId;
  /** The card selected on the setup style step; Balanced until the user picks another. */
  readonly style: SetupPreset;
  /** The step was opened with Edit on the summary: Next and Back return to the summary. */
  readonly editingFromSummary: boolean;
  /** Steps finished with Next (the rail shows a check); skipped steps are not in here. */
  readonly done: readonly WizardStepId[];
}

/** What the primary button does: Next, "Finish setup" (presets), or the summary's final button. */
export type PrimaryButton = "next" | "finishSetup" | "startMumcord" | "finish";

export function createWizardFlow(options: WizardFlowOptions): WizardFlowState {
  const state: WizardFlowState = {
    mode: options.mode,
    mumbleFound: options.mode === "firstStart" && (options.mumbleFound ?? false),
    includeWelcome: options.mode === "firstStart" || (options.includeWelcome ?? true),
    step: "welcome",
    style: RECOMMENDED_PRESET,
    editingFromSummary: false,
    done: [],
  };
  const first = wizardPath(state)[0];
  return first === undefined ? state : { ...state, step: first };
}

/** The steps of the current path in order, from the mode, the import and the setup style. */
export function wizardPath(state: WizardFlowState): WizardStepId[] {
  const path: WizardStepId[] = [];
  if (state.includeWelcome) path.push("welcome");
  if (state.mode === "firstStart") {
    if (state.mumbleFound) path.push("import");
    path.push("identity");
  }
  path.push("voice", "setupStyle");
  if (state.style === "custom") path.push(...CUSTOM_STEPS);
  path.push("summary");
  return path;
}

/** 1-based number of the current step for "STEP n OF m". */
export function stepNumber(state: WizardFlowState): number {
  return wizardPath(state).indexOf(state.step) + 1;
}

/** The m of "STEP n OF m": the steps of the current path, summary included. */
export function stepTotal(state: WizardFlowState): number {
  return wizardPath(state).length;
}

/** Share of the path done, 0 – 1, for the rail's progress bar. */
export function progress(state: WizardFlowState): number {
  const total = stepTotal(state);
  return total <= 1 ? 1 : (stepNumber(state) - 1) / (total - 1);
}

export interface RailEntry {
  id: WizardStepId;
  /** Position in the current path; `null` for Custom steps while a preset is selected. */
  number: number | null;
  /** Tagged "Custom" in the rail. */
  custom: boolean;
  state: "done" | "current" | "upcoming";
}

/**
 * The rail's ordered list: every step of the path, plus the Custom steps (unnumbered) while a
 * preset is selected, so the rail always shows what Custom would add.
 */
export function railEntries(state: WizardFlowState): RailEntry[] {
  const path = wizardPath(state);
  const all = state.style === "custom" ? path : withCustomSteps(path);
  return all.map((id) => {
    const index = path.indexOf(id);
    return {
      id,
      number: index === -1 ? null : index + 1,
      custom: CUSTOM_STEPS.includes(id),
      state: id === state.step ? "current" : state.done.includes(id) ? "done" : "upcoming",
    };
  });
}

function withCustomSteps(path: readonly WizardStepId[]): WizardStepId[] {
  const at = path.indexOf("setupStyle") + 1;
  return [...path.slice(0, at), ...CUSTOM_STEPS, ...path.slice(at)];
}

export function canSkip(state: WizardFlowState): boolean {
  return (
    SKIPPABLE_STEPS.includes(state.step) || (state.mode === "rerun" && state.step === "welcome")
  );
}

export function canGoBack(state: WizardFlowState): boolean {
  return state.editingFromSummary || stepNumber(state) > 1;
}

/** Cancel (nothing saved) exists only in the rerun (#149). */
export function canCancel(state: WizardFlowState): boolean {
  return state.mode === "rerun";
}

export function primaryButton(state: WizardFlowState): PrimaryButton {
  if (state.step === "summary") return state.mode === "rerun" ? "finish" : "startMumcord";
  if (state.step === "setupStyle" && state.style !== "custom") return "finishSetup";
  return "next";
}

/** Picks a card on the setup style step; it decides whether the Custom steps follow. */
export function selectStyle(state: WizardFlowState, style: SetupPreset): WizardFlowState {
  return { ...state, style, done: state.done.filter((id) => !CUSTOM_STEPS.includes(id)) };
}

/** Next (or Finish setup): the following step. The summary's own button finishes the wizard. */
export function next(state: WizardFlowState): WizardFlowState {
  if (state.step === "summary") return state;
  const done = state.done.includes(state.step) ? state.done : [...state.done, state.step];
  return { ...advance(state), done };
}

/** Skip: like Next on an optional step, but the step gets no check. */
export function skip(state: WizardFlowState): WizardFlowState {
  return canSkip(state) ? advance(state) : state;
}

function advance(state: WizardFlowState): WizardFlowState {
  // A new setup style decides the rest of the path, so an edit of it runs on from there
  // (Custom walks its steps) instead of returning to the summary at once.
  if (state.editingFromSummary && state.step !== "setupStyle") {
    return { ...state, step: "summary", editingFromSummary: false };
  }
  const path = wizardPath(state);
  const following = path[path.indexOf(state.step) + 1] ?? "summary";
  return { ...state, step: following, editingFromSummary: false };
}

/** Back: the previous step of the path, or the summary while editing from it. */
export function back(state: WizardFlowState): WizardFlowState {
  if (state.editingFromSummary) return { ...state, step: "summary", editingFromSummary: false };
  const path = wizardPath(state);
  const previous = path[path.indexOf(state.step) - 1];
  return previous === undefined ? state : { ...state, step: previous };
}

/** Edit on a summary card: opens that step; Next or Back then return to the summary. */
export function editFromSummary(state: WizardFlowState, step: WizardStepId): WizardFlowState {
  if (state.step !== "summary" || step === "summary" || !wizardPath(state).includes(step)) {
    return state;
  }
  return { ...state, step, editingFromSummary: true };
}
