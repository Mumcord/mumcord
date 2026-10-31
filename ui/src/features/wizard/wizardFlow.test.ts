import { describe, expect, it } from "vitest";
import {
  back,
  canCancel,
  canGoBack,
  canSkip,
  createWizardFlow,
  editFromSummary,
  next,
  primaryButton,
  progress,
  railEntries,
  selectStyle,
  skip,
  stepNumber,
  stepTotal,
  wizardPath,
  type WizardFlowState,
  type WizardStepId,
} from "./wizardFlow";

const CUSTOM: WizardStepId[] = [
  "privateMessages",
  "connections",
  "features",
  "appearance",
  "startup",
];

/** Presses Next until the summary, recording every step on the way. */
function walk(state: WizardFlowState): WizardStepId[] {
  const seen: WizardStepId[] = [state.step];
  let current = state;
  while (current.step !== "summary") {
    current = next(current);
    seen.push(current.step);
  }
  return seen;
}

function at(state: WizardFlowState, step: WizardStepId): WizardFlowState {
  let current = state;
  while (current.step !== step) {
    const following = next(current);
    if (following.step === current.step) throw new Error(`never reached ${step}`);
    current = following;
  }
  return current;
}

describe("first start", () => {
  const first = createWizardFlow({ mode: "firstStart" });

  it("starts at the welcome step with Balanced selected", () => {
    expect(first.step).toBe("welcome");
    expect(first.style).toBe("balanced");
    expect(canCancel(first)).toBe(false);
    expect(canGoBack(first)).toBe(false);
  });

  it("jumps from the setup style to the summary for each preset", () => {
    for (const style of ["everythingOn", "balanced", "maximumPrivacy"] as const) {
      const state = selectStyle(first, style);
      expect(walk(state)).toEqual(["welcome", "identity", "voice", "setupStyle", "summary"]);
      expect(primaryButton(at(state, "setupStyle"))).toBe("finishSetup");
      expect(stepTotal(state)).toBe(5);
    }
  });

  it("walks the detailed steps for Custom", () => {
    const state = selectStyle(first, "custom");
    expect(walk(state)).toEqual([
      "welcome",
      "identity",
      "voice",
      "setupStyle",
      ...CUSTOM,
      "summary",
    ]);
    expect(primaryButton(at(state, "setupStyle"))).toBe("next");
    expect(stepTotal(state)).toBe(10);
  });

  it("puts the Mumble import before the identity when a profile was found", () => {
    const state = createWizardFlow({ mode: "firstStart", mumbleFound: true });
    expect(wizardPath(state)).toEqual([
      "welcome",
      "import",
      "identity",
      "voice",
      "setupStyle",
      "summary",
    ]);
    expect(stepNumber(at(state, "identity"))).toBe(3);
  });

  it("always shows the welcome step", () => {
    expect(createWizardFlow({ mode: "firstStart", includeWelcome: false }).step).toBe("welcome");
  });

  it("ends with Start Mumcord on the summary, where Next does nothing", () => {
    const summary = at(first, "summary");
    expect(primaryButton(summary)).toBe("startMumcord");
    expect(next(summary)).toBe(summary);
    expect(primaryButton(first)).toBe("next");
  });
});

describe("rerun", () => {
  it("skips import and identity", () => {
    const state = createWizardFlow({ mode: "rerun", mumbleFound: true });
    expect(walk(state)).toEqual(["welcome", "voice", "setupStyle", "summary"]);
    expect(walk(selectStyle(state, "custom"))).toEqual([
      "welcome",
      "voice",
      "setupStyle",
      ...CUSTOM,
      "summary",
    ]);
  });

  it("can leave out the welcome step or skip it", () => {
    const without = createWizardFlow({ mode: "rerun", includeWelcome: false });
    expect(without.step).toBe("voice");
    expect(stepTotal(without)).toBe(3);
    const withWelcome = createWizardFlow({ mode: "rerun" });
    expect(canSkip(withWelcome)).toBe(true);
    const skipped = skip(withWelcome);
    expect(skipped.step).toBe("voice");
    expect(skipped.done).toEqual([]);
  });

  it("offers Cancel and finishes with Finish", () => {
    const state = createWizardFlow({ mode: "rerun" });
    expect(canCancel(state)).toBe(true);
    expect(primaryButton(at(state, "summary"))).toBe("finish");
  });
});

describe("back and skip", () => {
  it("goes back along the path", () => {
    const custom = selectStyle(createWizardFlow({ mode: "firstStart" }), "custom");
    const features = at(custom, "features");
    expect(back(features).step).toBe("connections");
    expect(back(at(custom, "summary")).step).toBe("startup");
    expect(back(at(createWizardFlow({ mode: "firstStart" }), "summary")).step).toBe("setupStyle");
    const welcome = createWizardFlow({ mode: "firstStart" });
    expect(back(welcome)).toBe(welcome);
  });

  it("skips only optional steps, without a check", () => {
    const custom = selectStyle(createWizardFlow({ mode: "firstStart" }), "custom");
    for (const step of ["welcome", "identity", "voice", "setupStyle", "privateMessages"] as const) {
      const state = at(custom, step);
      expect(canSkip(state)).toBe(false);
      expect(skip(state)).toBe(state);
    }
    const features = at(custom, "features");
    expect(canSkip(features)).toBe(true);
    const appearance = skip(features);
    expect(appearance.step).toBe("appearance");
    expect(appearance.done).not.toContain("features");
    expect(skip(appearance).step).toBe("startup");
  });
});

describe("edit from the summary", () => {
  it("returns to the summary after Next", () => {
    const summary = at(selectStyle(createWizardFlow({ mode: "firstStart" }), "custom"), "summary");
    for (const step of ["welcome", "identity", "voice", ...CUSTOM] as WizardStepId[]) {
      const editing = editFromSummary(summary, step);
      expect(editing.step).toBe(step);
      expect(canGoBack(editing)).toBe(true);
      expect(next(editing).step).toBe("summary");
      expect(next(editing).editingFromSummary).toBe(false);
    }
  });

  it("returns to the summary on Back and Skip", () => {
    const summary = at(selectStyle(createWizardFlow({ mode: "firstStart" }), "custom"), "summary");
    expect(back(editFromSummary(summary, "connections")).step).toBe("summary");
    expect(skip(editFromSummary(summary, "features")).step).toBe("summary");
  });

  it("walks the Custom steps when the setup style changes to Custom", () => {
    const summary = at(createWizardFlow({ mode: "firstStart" }), "summary");
    const editing = selectStyle(editFromSummary(summary, "setupStyle"), "custom");
    expect(walk(editing)).toEqual(["setupStyle", ...CUSTOM, "summary"]);
  });

  it("returns to the summary when a preset is picked again", () => {
    const summary = at(selectStyle(createWizardFlow({ mode: "firstStart" }), "custom"), "summary");
    const editing = selectStyle(editFromSummary(summary, "setupStyle"), "maximumPrivacy");
    expect(primaryButton(editing)).toBe("finishSetup");
    expect(next(editing).step).toBe("summary");
  });

  it("ignores steps outside the path and edits from other steps", () => {
    const summary = at(createWizardFlow({ mode: "rerun" }), "summary");
    expect(editFromSummary(summary, "identity")).toBe(summary);
    expect(editFromSummary(summary, "features")).toBe(summary);
    expect(editFromSummary(summary, "summary")).toBe(summary);
    const voice = at(createWizardFlow({ mode: "rerun" }), "voice");
    expect(editFromSummary(voice, "voice")).toBe(voice);
  });
});

describe("rail", () => {
  it("numbers the path, shows Custom steps unnumbered for presets and marks progress", () => {
    const style = at(createWizardFlow({ mode: "firstStart" }), "setupStyle");
    expect(stepNumber(style)).toBe(4);
    expect(progress(style)).toBe(3 / 4);
    expect(railEntries(style)).toEqual([
      { id: "welcome", number: 1, custom: false, state: "done" },
      { id: "identity", number: 2, custom: false, state: "done" },
      { id: "voice", number: 3, custom: false, state: "done" },
      { id: "setupStyle", number: 4, custom: false, state: "current" },
      ...CUSTOM.map((id) => ({ id, number: null, custom: true, state: "upcoming" })),
      { id: "summary", number: 5, custom: false, state: "upcoming" },
    ]);
  });

  it("numbers the Custom steps once Custom is selected", () => {
    const style = selectStyle(at(createWizardFlow({ mode: "firstStart" }), "setupStyle"), "custom");
    expect(railEntries(style).map((entry) => entry.number)).toEqual([
      1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
    ]);
    expect(progress(createWizardFlow({ mode: "firstStart" }))).toBe(0);
    expect(progress(at(style, "summary"))).toBe(1);
  });

  it("drops the Custom checks when a preset replaces Custom", () => {
    const custom = selectStyle(createWizardFlow({ mode: "firstStart" }), "custom");
    const summary = at(custom, "summary");
    expect(summary.done).toContain("connections");
    const editing = selectStyle(editFromSummary(summary, "setupStyle"), "balanced");
    expect(editing.done).not.toContain("connections");
    expect(editing.done).toContain("voice");
  });
});
