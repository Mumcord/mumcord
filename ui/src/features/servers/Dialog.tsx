import { useEffect, useEffectEvent, useId, useRef, type ReactNode, type RefObject } from "react";

interface DialogProps {
  title: string;
  onClose: () => void;
  children: ReactNode;
  /** "alertdialog" for blocking warnings such as a changed certificate. */
  role?: "dialog" | "alertdialog";
  /** Element focused when the dialog opens; defaults to the first form control. */
  initialFocus?: RefObject<HTMLElement | null> | undefined;
  describedBy?: string | undefined;
  /** Extra class on the `<dialog>`, for a dialog that needs its own size or layout. */
  className?: string | undefined;
}

const FOCUSABLE = "input, select, textarea, button";

/**
 * Modal dialog on the native `<dialog>` element: `showModal()` makes the rest of the window inert
 * and keeps Tab inside. Escape closes; focus returns to the element that opened it.
 */
export function Dialog({
  title,
  onClose,
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
      <h2 id={titleId} className="servers-dialog__title">
        {title}
      </h2>
      {children}
    </dialog>
  );
}
