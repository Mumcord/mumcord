/**
 * How long a party may report no progress before a missing decoder is named (FR-13.35). YouTube
 * answers a missing decoder with its own panel and never sends `onError`, so a picture that stays
 * at zero is the only signal the app gets; ten seconds are long enough for a slow stream to start.
 */
export const STALL_MS = 10_000;
