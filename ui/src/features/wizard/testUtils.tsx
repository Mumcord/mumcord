import { render } from "@testing-library/react";
import type { ReactNode } from "react";
import { vi, type Mock } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../api";
import { PreferencesContext, type Preferences } from "../../app/preferences";
import { ViewContext, type View } from "../../app/view";
import { applyLanguage } from "../../i18n";
import { applyThemeToDocument } from "../../theme/theme";

export interface Harness {
  api: MockApi;
  finishWizard: Mock<View["finishWizard"]>;
  applySettings: Mock<Preferences["applySettings"]>;
}

/** Fresh mock engine (first start) plus the view/preferences contexts the wizard needs. */
export async function setupWizardTest(theme: "dark" | "light" = "dark"): Promise<Harness> {
  await applyLanguage("en");
  applyThemeToDocument(theme);
  const api = createMockApi({ seed: "fresh" });
  setApi(api);
  return {
    api,
    finishWizard: vi.fn<View["finishWizard"]>(),
    applySettings: vi.fn<Preferences["applySettings"]>(),
  };
}

export function renderInWizard(harness: Harness, ui: ReactNode) {
  const view: View = {
    screen: "wizard",
    overlay: null,
    finishWizard: harness.finishWizard,
    openSettings: vi.fn(),
    closeOverlay: vi.fn(),
    retry: vi.fn(),
  };
  const preferences: Preferences = {
    theme: "system",
    resolvedTheme: "dark",
    language: "en",
    chatDisplay: "cozy",
    linkPreviews: true,
    applySettings: harness.applySettings,
  };
  return render(
    <ViewContext.Provider value={view}>
      <PreferencesContext.Provider value={preferences}>
        <main aria-label="Mumcord">
          <h1>Welcome to Mumcord</h1>
          {ui}
        </main>
      </PreferencesContext.Provider>
    </ViewContext.Provider>,
  );
}
