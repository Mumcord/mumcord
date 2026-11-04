import { useEffect, useEffectEvent, useId, useRef, type ReactNode, type RefObject } from "react";

interface DialogProps {
  title: string;
  /** Shown before the title text, hidden from screen readers (the title already names it). */
  titleLead?: ReactNode;
  onClose: () => void;
  /** Accessible name of a visible close button (×) in the header; no button when omitted. */
  closeLabel?: string | undefined;
  children: ReactNode;
  /** "alertdialog" for blocking warnings such as a changed certificate. */
  role?: "dialog" | "alertdialog";
  /** Element focused when the dialog opens; defaults to the first form control. */
  initialFocus?: RefObject<HTMLElement | null> | undefined;
  describedBy?: string | undefined;
  /** Extra class on the `<dialog>`, for a dialog that needs its own size or layout. */
  className?: string | undefined;
}

// The header close button is skipped so opening the dialog does not park focus on "close".
const FOCUSABLE = "input, select, textarea, button:not(.servers-dialog__close)";

/**
 * Modal dialog on the native `<dialog>` element: `showModal()` makes the rest of the window inert
 * and keeps Tab inside. Escape closes; focus returns to the element that opened it.
 */
export function Dialog({
  title,
  titleLead,
  onClose,
  closeLabel,
  children,
  role = "dialog",
  initialFocus,
  describedBy,
  className,
}: DialogProps) {
  const titleId = useId();
  const ref = useRef<HTMLDialogElement>(null);
  const requestClose = useEffectEvent(() => {
    onClose();
  });

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.showModal();
    const target = initialFocus?.current ?? dialog.querySelector<HTMLElement>(FOCUSABLE);
    target?.focus();
    const onCancel = (event: Event) => {
      // Keep React in charge of closing so state and DOM never disagree.
      event.preventDefault();
      requestClose();
    };
    dialog.addEventListener("cancel", onCancel);
    return () => {
      dialog.removeEventListener("cancel", onCancel);
      if (dialog.open) dialog.close();
      if (opener?.isConnected) opener.focus();
    };
  }, [initialFocus]);

  return (
    <dialog
      ref={ref}
      className={className ? `servers-dialog ${className}` : "servers-dialog"}
      role={role === "alertdialog" ? "alertdialog" : undefined}
      aria-modal="true"
      aria-labelledby={titleId}
      aria-describedby={describedBy}
    >
      {closeLabel === undefined ? (
        <h2 id={titleId} className="servers-dialog__title">
          {titleLead}
          {title}
        </h2>
      ) : (
        <div className="servers-dialog__head">
          <h2 id={titleId} className="servers-dialog__title">
            {titleLead}
            {title}
          </h2>
          <button
            type="button"
            className="servers-dialog__close"
            aria-label={closeLabel}
            data-tooltip={closeLabel}
            onClick={onClose}
          >
            <span aria-hidden="true">✕</span>
          </button>
        </div>
      )}
      {children}
    </dialog>
  );
}
