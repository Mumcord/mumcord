import "./disclosure.css";
import { useId, useState, type ReactNode } from "react";

/**
 * One row with a chevron that shows or hides its content. Starts collapsed; the content is only
 * mounted while open. A native button gives click and Enter/Space.
 */
export function Disclosure({
  title,
  hint,
  children,
}: {
  title: string;
  /** Short state text shown beside the title while collapsed. */
  hint?: string | undefined;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const panelId = useId();
  return (
    <div className="disclosure">
      <button
        type="button"
        className="disclosure__toggle"
        aria-expanded={open}
        aria-controls={open ? panelId : undefined}
        onClick={() => {
          setOpen(!open);
        }}
      >
        <svg
          className="disclosure__chevron"
          viewBox="0 0 24 24"
          width="14"
          height="14"
          aria-hidden="true"
          focusable="false"
        >
          <path
            d="M9 6l6 6l-6 6"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
        <span className="disclosure__title">{title}</span>
        {!open && hint && <span className="disclosure__hint">{hint}</span>}
      </button>
      {open && (
        <div id={panelId} className="disclosure__panel">
          {children}
        </div>
      )}
    </div>
  );
}
