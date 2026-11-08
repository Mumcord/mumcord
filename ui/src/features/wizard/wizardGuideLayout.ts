import type { AaronPose } from "../tips/Aaron";
import type { WizardStepId } from "./wizardFlow";

/**
 * What Aaron (#637) points at on each wizard step: a CSS selector for the field the step is
 * about, searched inside the current step. `null` (or no match) lets him stand idle. The
 * bubble's title lives at `wizard:guide.titles.<step>`, its hint at `wizard:guide.hints.<step>`.
 */
export const GUIDE_TARGETS: Record<WizardStepId, string | null> = {
  welcome: null,
  import: ".mumble-import__list",
  identity: ".wizard-identity-card",
  voice: ".wizard-voice__devices select",
  setupStyle: ".preset-cards",
  privateMessages: ".wizard-rows",
  notifications: ".wizard-rows",
  connections: ".wizard-card",
  features: ".wizard-grid",
  appearance: ".wizard-segmented",
  startup: ".wizard-rows",
  summary: ".wizard-summary-card",
};

export interface Box {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/**
 * Aaron's pose for a target: he points sideways when the target lies beside him, up when it is
 * above him, and stands idle without a target or when it is right under or behind him.
 */
export function poseFor(target: Box | null, aaron: Box): AaronPose {
  if (target === null) return "idle";
  const centerX = (target.left + target.right) / 2;
  const centerY = (target.top + target.bottom) / 2;
  if (centerX < aaron.left) return "point-left";
  if (centerX > aaron.right) return "point-right";
  if (centerY < aaron.top) return "point-up";
  return "idle";
}

/**
 * How far down his column Aaron stands, so his pointing arm (`armY` below his top) is level with
 * the target's middle while he and his bubble (`height`) stay inside the column.
 */
export function guideOffset(target: Box | null, column: Box, armY: number, height: number): number {
  if (target === null) return 0;
  const wanted = (target.top + target.bottom) / 2 - column.top - armY;
  const lowest = Math.max(0, column.bottom - column.top - height);
  return Math.min(Math.max(wanted, 0), lowest);
}

/**
 * How far Aaron steps left out of his column towards the target (zero or negative). He stands
 * just right of the target, but never over anything else of the step (`beside`: the boxes next
 * to the target at his height), and never further right than his column.
 */
export function guideShift(target: Box | null, beside: Box[], column: Box, gap: number): number {
  if (target === null) return 0;
  const contentRight = Math.max(target.right, ...beside.map((box) => box.right));
  return Math.min(0, contentRight + gap - column.left);
}

/** Where Aaron stands: how far down his column, and how far out of it towards the target. */
export interface GuideSpot {
  offset: number;
  shift: number;
}

/**
 * Picks Aaron's spot. His arm starts level with the target's middle. When content beside the
 * target above his arm blocks him from stepping close, he tries standing just below that
 * content, as long as his arm still points at the target, and takes whichever spot is closer.
 * `besideAt(top, bottom)` reports the step's content beside the target between two heights.
 */
export function guideSpot(
  target: Box | null,
  column: Box,
  armY: number,
  height: number,
  gap: number,
  besideAt: (top: number, bottom: number) => Box[],
): GuideSpot {
  const offset = guideOffset(target, column, armY, height);
  if (target === null) return { offset, shift: 0 };
  const shiftAt = (at: number) =>
    guideShift(target, besideAt(column.top + at, column.top + at + height), column, gap);
  const shift = shiftAt(offset);
  const arm = column.top + offset + armY;
  const above = besideAt(column.top + offset, column.top + offset + height).filter(
    (box) => box.top < arm,
  );
  if (above.length === 0) return { offset, shift };
  const lower = Math.max(...above.map((box) => box.bottom)) + gap - column.top;
  const lowest = Math.max(0, column.bottom - column.top - height);
  if (lower > lowest || column.top + lower + armY > target.bottom - gap) return { offset, shift };
  const lowerShift = shiftAt(lower);
  return lowerShift < shift ? { offset: lower, shift: lowerShift } : { offset, shift };
}
