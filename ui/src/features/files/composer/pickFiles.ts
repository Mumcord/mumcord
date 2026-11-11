/**
 * The system file picker for the paperclip (several files). The engine opens it, so only paths
 * the user really picked can become attachments. With the mock engine (browser dev) there is no
 * native dialog, so it asks for one path with `window.prompt`.
 */
import { getApi } from "../../../api";

/** Paths the user chose; empty when the picker was cancelled. */
export async function pickFiles(title: string): Promise<string[]> {
  if (getApi().kind !== "tauri") {
    const answer = window.prompt(title, "");
    return answer === null || answer.trim() === "" ? [] : [answer.trim()];
  }
  return getApi().filesPick(title);
}
