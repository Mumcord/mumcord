/**
 * Finds the on-screen control for a search hit inside the settings panel. Controls are found by
 * their visible label (inside their group, because labels like "Position" repeat), so the
 * section components need no extra wiring.
 */
export interface JumpTarget {
  /** Receives keyboard focus. */
  focus: HTMLElement;
  /** Gets the short highlight. */
  highlight: HTMLElement;
}

const FOCUSABLE = "input:not([disabled]), select:not([disabled]), button:not([disabled]), textarea";

const sameText = (element: Element, text: string) => element.textContent.trim() === text.trim();

export function findGroup(panel: HTMLElement, title: string): HTMLElement | null {
  for (const group of panel.querySelectorAll<HTMLElement>(".settings-group")) {
    const heading = group.querySelector(":scope > .settings-group__title");
    if (heading && sameText(heading, title)) return group;
  }
  return null;
}

function controlTarget(scope: HTMLElement, label: string): JumpTarget | null {
  for (const element of scope.querySelectorAll<HTMLElement>("label, legend, button")) {
    if (!sameText(element, label)) continue;
    if (element instanceof HTMLButtonElement) return { focus: element, highlight: element };
    if (element instanceof HTMLLabelElement) {
      const control =
        element.control ?? element.parentElement?.querySelector<HTMLElement>(FOCUSABLE);
      if (!control) continue;
      const row =
        element.closest<HTMLElement>(".settings-switch, .settings-field-row") ??
        element.parentElement ??
        control;
      return { focus: control, highlight: row };
    }
    const fieldset = element.closest("fieldset");
    const radio =
      fieldset?.querySelector<HTMLElement>("input:checked") ??
      fieldset?.querySelector<HTMLElement>(FOCUSABLE);
    if (fieldset && radio) return { focus: radio, highlight: fieldset };
  }
  return null;
}

/** The control, or for a group-only entry the group's first control; null while not rendered. */
export function findTarget(
  panel: HTMLElement,
  group: string | undefined,
  control: string | undefined,
): JumpTarget | null {
  const scope = group === undefined ? panel : findGroup(panel, group);
  if (!scope) return null;
  if (control !== undefined) return controlTarget(scope, control);
  const first = scope.querySelector<HTMLElement>(FOCUSABLE);
  return { focus: first ?? headingOf(scope), highlight: scope };
}

/** A heading made focusable, for when there is no control to land on. */
export function headingOf(scope: HTMLElement): HTMLElement {
  const heading = scope.querySelector<HTMLElement>("h2, h3") ?? scope;
  heading.tabIndex = -1;
  return heading;
}
