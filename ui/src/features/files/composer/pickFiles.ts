/**
 * The system file picker for the paperclip (several files). With the mock engine (browser dev)
 * there is no native dialog, so it asks for one path with `window.prompt`.
 */
import { open } from "@tauri-apps/plugin-dialog";
import { getApi } from "../../../api";

/** Paths the user chose; empty when the picker was cancelled. */
export async function pickFiles(title: string): Promise<string[]> {
  if (getApi().kind !== "tauri") {
    const answer = window.prompt(title, "");
    return answer === null || answer.trim() === "" ? [] : [answer.trim()];
  }
  const picked = await open({ title, multiple: true, directory: false });
  return picked ?? [];
}
