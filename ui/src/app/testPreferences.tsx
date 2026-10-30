import { render, type RenderResult } from "@testing-library/react";
import type { ReactElement, ReactNode } from "react";
import { PreferencesContext, type Preferences } from "./preferences";
import { ViewContext, type View } from "./view";

/** Test wrapper that provides the preferences with "Show link previews and embeds" on or off. */
export function withEmbeds(linkPreviews: boolean, view?: Pick<View, "openPrivacySettings">) {
  const preferences = { linkPreviews } as Preferences;
  return function EmbedsWrapper({ children }: { children: ReactNode }) {
    return (
      <PreferencesContext value={preferences}>
        <ViewContext value={(view ?? null) as View | null}>{children}</ViewContext>
      </PreferencesContext>
    );
  };
}

/** Renders `ui` with embedded players on, as in a default install. */
export function renderWithEmbeds(ui: ReactElement): RenderResult {
  return render(ui, { wrapper: withEmbeds(true) });
}
