import type { AriaRole } from "react";

/**
 * Small "?" button that explains the control next to it. The text appears as a tooltip on hover and
 * keyboard focus (and is announced through `aria-describedby`, see `TooltipLayer`); Escape closes it.
 */
export function InfoTip({
  label,
  text,
  tabIndex,
  role,
  placement,
}: {
  label: string;
  text: string;
  /** Lets a roving-focus parent keep the tip out of the Tab order and reach it with an arrow key. */
  tabIndex?: number;
  /** Inside a menu, a focusable control has to be a menu item to be reachable there. */
  role?: AriaRole;
  /** Preferred side for the tooltip; the shared layer flips it when there is no room. */
  placement?: "top" | "bottom" | "left" | "right";
}) {
  return (
    <button
      type="button"
      role={role}
      className="info-tip"
      aria-label={label}
      tabIndex={tabIndex}
      data-tooltip={text}
      data-tooltip-placement={placement}
    >
      ?
    </button>
  );
}
