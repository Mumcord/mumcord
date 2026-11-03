import { useEffect, useEffectEvent, useId, useRef, type ReactNode, type RefObject } from "react";
import "../../app/app.css";
import "./fileDialogs.css";

interface FileDialogProps {
  title: string;
  /** Shown left of the title, e.g. the warning sign of the risky dialog. */
  icon?: ReactNode;
  role?: "dialog" | "alertdialog";
  /** Focused when the dialog opens. */
  initialFocus: RefObject<HTMLButtonElement | null>;
  describedBy?: string | undefined;
  onClose: () => void;
  children: ReactNode;
}

/**
 * The file sharing dialogs in the style of the other app dialogs (user decision 6): a native modal
 * `<dialog>` keeps Tab inside, Escape closes, and focus returns to the card button that opened it.
 */
export function FileDialog({
  title,
  icon,
  role = "dialog",
  initialFocus,
  describedBy,
  onClose,
  children,
}: FileDialogProps) {
  const titleId = useId();
  const ref = useRef<HTMLDialogElement>(null);
  const close = useEffectEvent(() => {
    onClose();
  });

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.showModal();
    initialFocus.current?.focus();
    const onCancel = (event: Event) => {
      // React stays in charge of closing so state and DOM never disagree.
      event.preventDefault();
      close();
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
      className="app-dialog file-dialog"
      role={role === "alertdialog" ? "alertdialog" : undefined}
      aria-modal="true"
      aria-labelledby={titleId}
      aria-describedby={describedBy}
    >
      <div className="file-dialog__head">
        {icon}
        <h2 id={titleId} className="app-dialog__title">
          {title}
        </h2>
      </div>
      {children}
    </dialog>
  );
}
