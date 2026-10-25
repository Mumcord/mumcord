import type { ReactNode } from "react";

export interface CollapsibleSectionProps {
  /** Builds the aria ids; the same id goes to `useSectionOpen`. */
  id: string;
  title: ReactNode;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  className?: string | undefined;
  /** Only called while open, so a collapsed section costs nothing to render. */
  children: () => ReactNode;
}

/** Section with a collapsible header, shared by the member column sections and Friends. */
export function CollapsibleSection({
  id,
  title,
  open,
  onOpenChange,
  className,
  children,
}: CollapsibleSectionProps) {
  return (
    <section className={`collapsible-section ${className ?? ""}`} aria-labelledby={`${id}-title`}>
      <h2 id={`${id}-title`}>
        <button
          type="button"
          className="collapsible-section__toggle"
          aria-expanded={open}
          aria-controls={`${id}-items`}
          onClick={() => {
            onOpenChange(!open);
          }}
        >
          <span aria-hidden="true">{open ? "▾" : "▸"}</span> {title}
        </button>
      </h2>
      <div id={`${id}-items`} hidden={!open}>
        {open && children()}
      </div>
    </section>
  );
}
