import { useEffect, useId, useRef, type ReactNode } from "react";

const FOCUSABLE = 'button, input, textarea, [href], [tabindex]:not([tabindex="-1"])';

interface ModalProps {
  title: string;
  onClose: () => void;
  children: ReactNode;
  /** A line under the title. */
  subtitle?: string | null;
  /** Shown before the title, e.g. a plugin's icon; decorative. */
  icon?: ReactNode;
  /** If set, the header gets a ✕ button with this accessible name. */
  closeLabel?: string;
  /** Extra class on the dialog box, e.g. to size it for its content. */
  className?: string | undefined;
}

/** Small modal dialog: Escape closes, Tab stays inside, focus starts on the first control. */
export function Modal({
  title,
  onClose,
  children,
  subtitle,
  icon,
  closeLabel,
  className,
}: ModalProps) {
  const titleId = useId();
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    // Focus starts on the content, not on the header's close button.
    const first = [...dialog.querySelectorAll<HTMLElement>(FOCUSABLE)].find(
      (el) => !el.hasAttribute("data-modal-close"),
    );
    (first ?? dialog).focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        onClose();
        return;
      }
      if (event.key !== "Tab") return;
      const items = [...dialog.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
        (el) => !el.hasAttribute("disabled"),
      );
      const firstItem = items[0];
      const lastItem = items[items.length - 1];
      if (!firstItem || !lastItem) return;
      if (event.shiftKey && document.activeElement === firstItem) {
        event.preventDefault();
        lastItem.focus();
      } else if (!event.shiftKey && document.activeElement === lastItem) {
        event.preventDefault();
        firstItem.focus();
      }
    };
    dialog.addEventListener("keydown", onKeyDown);
    // Content that re-renders (e.g. a plugin's setup form turning into its player) can remove the
    // focused control; the focus would fall to the page and Escape and Tab would stop working.
    const keepFocus = new MutationObserver(() => {
      if (document.activeElement === document.body || document.activeElement === null) {
        dialog.focus();
      }
    });
    keepFocus.observe(dialog, { childList: true, subtree: true });
    return () => {
      dialog.removeEventListener("keydown", onKeyDown);
      keepFocus.disconnect();
    };
  }, [onClose]);

  return (
    <div className="userpanel-modal">
      <div
        ref={ref}
        className={className ? `userpanel-modal__dialog ${className}` : "userpanel-modal__dialog"}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
      >
        {closeLabel === undefined && icon === undefined && !subtitle ? (
          <h2 id={titleId}>{title}</h2>
        ) : (
          <div className="userpanel-modal__head">
            {icon}
            <div className="userpanel-modal__titles">
              <h2 id={titleId}>{title}</h2>
              {subtitle ? <p className="userpanel-modal__subtitle">{subtitle}</p> : null}
            </div>
            {closeLabel === undefined ? null : (
              <button
                type="button"
                className="userpanel-modal__close"
                aria-label={closeLabel}
                data-tooltip={closeLabel}
                data-modal-close=""
                onClick={onClose}
              >
                <span aria-hidden="true">✕</span>
              </button>
            )}
          </div>
        )}
        {children}
      </div>
    </div>
  );
}
