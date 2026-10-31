import { vi } from "vitest";
import { initialViewState, type View } from "./view";

/** A view context for tests that render one screen or section without the app shell. */
export function fakeView(overrides: Partial<View> = {}): View {
  return {
    ...initialViewState,
    finishWizard: vi.fn(),
    openSettings: vi.fn(),
    openOverlaySettings: vi.fn(),
    openAudioSettings: vi.fn(),
    openPrivacySettings: vi.fn(),
    openRerunWizard: vi.fn(),
    closeRerunWizard: vi.fn(),
    closeOverlay: vi.fn(),
    retry: vi.fn(),
    ...overrides,
  };
}
