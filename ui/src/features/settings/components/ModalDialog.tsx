import {
  useContext,
  useId,
  useLayoutEffect,
  useRef,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { DialogHostContext } from "./dialogHost";

interface ModalDialogProps {
  title: string;
  onClose: () => void;
  /** Extra class on the dialog box, e.g. to make it wider. */
  className?: string;
  /** Optional icon and one-line subtitle shown beside the title. */
  icon?: ReactNode;
  subtitle?: ReactNode;
  children: ReactNode;
}

const FOCUSABLE =
  'input:not([disabled]), select:not([disabled]), textarea:not([disabled]), button:not([disabled]), [href], [tabindex]:not([tabindex="-1"])';

/**
 * Small modal on top of the settings overlay. The settings content behind it is inert, Tab
 * stays inside the dialog, Escape closes only this dialog (not the whole settings overlay) and
 * focus returns to the element that opened it.
 */
export function ModalDialog({
  title,
  onClose,
  className,
  icon,
  subtitle,
  children,
}: ModalDialogProps) {
  const titleId = useId();
  const ref = useRef<HTMLDivElement>(null);
  const host = useContext(DialogHostContext);

  useLayoutEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const close = host?.open(opener);
    ref.current?.querySelector<HTMLElement>(FOCUSABLE)?.focus();
    return () => {
      if (close) close();
      else opener?.focus();
    };
    // The dialog announces itself once; a new host value must not reopen it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onClose();
      return;
    }
    if (event.key !== "Tab" || !ref.current) return;
    const focusable = [...ref.current.querySelectorAll<HTMLElement>(FOCUSABLE)];
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (!first || !last) return;
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  return createPortal(
    <div className="settings-dialog__backdrop">
      {/* Escape and the Tab trap are handled for the whole dialog, wherever focus is inside it. */}
      {/* eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions */}
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className={className ? `settings-dialog ${className}` : "settings-dialog"}
        onKeyDown={onKeyDown}
      >
        {icon !== undefined || subtitle !== undefined ? (
          <div className="settings-dialog__header">
            {icon}
            <div>
              <h3 id={titleId} className="settings-dialog__title settings-dialog__title--tight">
                {title}
              </h3>
              {subtitle}
            </div>
          </div>
        ) : (
          <h3 id={titleId} className="settings-dialog__title">
            {title}
          </h3>
        )}
        {children}
      </div>
    </div>,
    host?.element ?? document.body,
  );
}
