/**
 * The right column panel both media views use (issue #163): a row of tabs and the content of the
 * chosen one. On a narrow window the same panel shows as the drawer over the chat, which the shell
 * already does for the member column.
 */
import type { ReactNode } from "react";
import "./media.css";

export interface PanelTab {
  id: string;
  label: string;
  /** A count shown with the label, e.g. "Playlist 5". */
  count?: number;
}

export interface MediaPanelProps {
  /** Accessible name of the panel. */
  label: string;
  tabs: readonly PanelTab[];
  active: string;
  onSelect: (id: string) => void;
  children: ReactNode;
}

export function MediaPanel({ label, tabs, active, onSelect, children }: MediaPanelProps) {
  return (
    <section className="media-panel" aria-label={label}>
      <div className="media-panel__tabs" role="tablist">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            type="button"
            role="tab"
            className="media-panel__tab"
            aria-selected={tab.id === active}
            onClick={() => {
              onSelect(tab.id);
            }}
          >
            {tab.label}
            {tab.count !== undefined && <span className="media-panel__count">{tab.count}</span>}
          </button>
        ))}
      </div>
      <div className="media-panel__body" role="tabpanel" aria-label={label}>
        {children}
      </div>
    </section>
  );
}
