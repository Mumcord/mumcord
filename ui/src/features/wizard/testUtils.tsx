import { render } from "@testing-library/react";
import type { ReactNode } from "react";
import { vi, type Mock } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../api";
import { PreferencesContext, type Preferences } from "../../app/preferences";
import { ViewContext, type View } from "../../app/view";
import { applyLanguage } from "../../i18n";
import { applyThemeToDocument } from "../../theme/theme";
import { StepNavContext, type StepNav } from "./stepNav";

export interface Harness {
  api: MockApi;
  finishWizard: Mock<View["finishWizard"]>;
  applySettings: Mock<Preferences["applySettings"]>;
}

/**
 * Mock engine plus the view/preferences contexts the wizard needs. "fresh" is a first start,
 * "ready" a finished setup like the one the rerun from Settings starts from (#149).
 */
export async function setupWizardTest(
  theme: "dark" | "light" = "dark",
  seed: "fresh" | "ready" = "fresh",
): Promise<Harness> {
  await applyLanguage("en");
  applyThemeToDocument(theme);
  const api = createMockApi({ seed });
  setApi(api);
  return {
    api,
    finishWizard: vi.fn<View["finishWizard"]>(),
    applySettings: vi.fn<Preferences["applySettings"]>(),
  };
}

/** A step's place in the flow when a step is rendered on its own: step 2 of 5, with Back. */
export const TEST_NAV: StepNav = {
  number: 2,
  total: 5,
  name: "Test",
  focusOnMount: false,
  primary: "next",
  onBack: () => undefined,
};

export function renderInWizard(harness: Harness, ui: ReactNode, nav: StepNav = TEST_NAV) {
  const view: View = {
    screen: "wizard",
    overlay: null,
    settingsSection: null,
    finishWizard: harness.finishWizard,
    openSettings: vi.fn(),
    openOverlaySettings: vi.fn(),
    openAudioSettings: vi.fn(),
    openPrivacySettings: vi.fn(),
    openRerunWizard: vi.fn(),
    closeRerunWizard: vi.fn(),
    rerunWizard: false,
    settingsRevision: 0,
    closeOverlay: vi.fn(),
    retry: vi.fn(),
  };
  const preferences: Preferences = {
    theme: "system",
    resolvedTheme: "dark",
    language: "en",
    chatDisplay: "cozy",
    linkPreviews: true,
    hideEmptyChannels: false,
    talkingFirst: false,
    showWeakConnectionIcon: true,
    lowLatency: false,
    applySettings: harness.applySettings,
  };
  return render(
    <ViewContext.Provider value={view}>
      <PreferencesContext.Provider value={preferences}>
        <main aria-label="Mumcord">
          <h1>Welcome to Mumcord</h1>
          <StepNavContext.Provider value={nav}>{ui}</StepNavContext.Provider>
        </main>
      </PreferencesContext.Provider>
    </ViewContext.Provider>,
  );
}
