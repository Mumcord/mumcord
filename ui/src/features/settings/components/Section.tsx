import type { ReactNode } from "react";

/** Common frame of a settings section: heading plus content. */
export function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="settings-section">
      <h2 className="settings-section__title">{title}</h2>
      {children}
    </div>
  );
}

/** Labelled group of controls inside a section. */
export function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="settings-group">
      <h3 className="settings-group__title">{title}</h3>
      {children}
    </div>
  );
}
