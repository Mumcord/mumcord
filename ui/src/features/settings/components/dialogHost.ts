import { createContext } from "react";

export interface DialogHost {
  /** Element the dialogs are rendered into; it sits outside the (then inert) settings content. */
  element: HTMLElement | null;
  /**
   * Announces an open dialog. The settings content behind it becomes inert; the returned
   * function closes it again and moves focus back to `opener` once the content is usable.
   */
  open: (opener: HTMLElement | null) => () => void;
}

export const DialogHostContext = createContext<DialogHost | null>(null);
