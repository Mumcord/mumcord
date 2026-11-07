/**
 * The window the app's tests pretend to run in: a website page unless a test says otherwise, which
 * the card reads through `appPageIsWebPage` (issue #419). Kept apart so `vi.mock` in a test file can
 * swap the answer without touching jsdom's `location`.
 */
export const appPage = { web: true };
