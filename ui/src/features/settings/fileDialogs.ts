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
  // Some native dialogs (GTK) append the filter's extension themselves, so offer the bare name and
  // add the extension afterwards only if it is still missing: exactly one extension everywhere.
  const ext = filter.extensions[0];
  const suffix = ext === undefined ? "" : `.${ext}`;
  const bare =
    suffix !== "" && defaultName.endsWith(suffix)
      ? defaultName.slice(0, -suffix.length)
      : defaultName;
  const path = await save({ title, defaultPath: bare, filters: [filter] });
  if (path === null || suffix === "") return path;
  const known = filter.extensions.some((e) => path.toLowerCase().endsWith(`.${e.toLowerCase()}`));
  return known ? path : `${path}${suffix}`;
}

/** Asks for an existing file; `null` when the user cancels. */
export async function pickOpenPath(filter: FileFilter, title: string): Promise<string | null> {
  if (getApi().kind !== "tauri") {
    return promptPath(title, `identity.${filter.extensions[0] ?? "p12"}`);
  }
  return open({ title, multiple: false, directory: false, filters: [filter] });
}
