import { openInBrowser } from "../../chat/openLink";

/** The score explanation document in the project repository (written by the docs task). */
export const SCORE_DOC_URL = "https://github.com/Mumcord/mumcord/blob/main/docs/plugin-score.md";

/** Opens the score explanation in the system browser; wired to "How the score is calculated". */
export function openScoreDoc(): void {
  openInBrowser(SCORE_DOC_URL);
}
