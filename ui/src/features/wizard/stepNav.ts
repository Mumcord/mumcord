import { createContext } from "react";
import type { PrimaryButton } from "./wizardFlow";

/** What the wizard tells the shown step about its place in the flow (issue #159). */
export interface StepNav {
  /** "STEP n OF m · NAME" */
  number: number;
  total: number;
  name: string;
  /** Move focus to the heading when the step appears (not on the very first render). */
  focusOnMount: boolean;
  /** The welcome step's button reads "Let's get started" instead of "Next". */
  primary: PrimaryButton | "getStarted";
  onBack?: (() => void) | undefined;
  /** Optional steps only: goes on without a check in the rail. */
  onSkip?: (() => void) | undefined;
  /** Rerun only (#149): closes the wizard without saving. */
  onCancel?: (() => void) | undefined;
}

export const StepNavContext = createContext<StepNav | null>(null);
