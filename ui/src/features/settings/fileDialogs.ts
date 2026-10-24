/**
 * Native file pickers. The engine API has no file dialog, so this uses the Tauri dialog plugin
 * directly; with the mock engine (browser dev) it asks for a path with `window.prompt`.
 */
import { open, save } from "@tauri-apps/plugin-dialog";
import { getApi } from "../../api";

export interface FileFilter {
  name: string;
  extensions: string[];
}

function promptPath(message: string, suggestion: string): string | null {
  const answer = window.prompt(message, suggestion);
  return answer === null || answer.trim() === "" ? null : answer.trim();
}

/** Asks where to save a file; `null` when the user cancels. */
export async function pickSavePath(
  defaultName: string,
  filter: FileFilter,
  title: string,
): Promise<string | null> {
  if (getApi().kind !== "tauri") return promptPath(title, defaultName);
  return save({ title, defaultPath: defaultName, filters: [filter] });
}

/** Asks for an existing file; `null` when the user cancels. */
export async function pickOpenPath(filter: FileFilter, title: string): Promise<string | null> {
  if (getApi().kind !== "tauri") {
    return promptPath(title, `identity.${filter.extensions[0] ?? "p12"}`);
  }
  return open({ title, multiple: false, directory: false, filters: [filter] });
}
