import { open } from "@tauri-apps/plugin-dialog";
import { getApi } from "../../../api";

/**
 * Asks for an existing folder; `null` when the user cancels. With the mock engine (browser dev)
 * it asks for a path with `window.prompt`, like the other pickers.
 */
export async function pickFolderPath(title: string): Promise<string | null> {
  if (getApi().kind !== "tauri") {
    const answer = window.prompt(title, "");
    return answer === null || answer.trim() === "" ? null : answer.trim();
  }
  const path = await open({ title, multiple: false, directory: true });
  return typeof path === "string" ? path : null;
}
