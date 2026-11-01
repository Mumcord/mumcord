import { getApi, toEngineError } from "../../../api";
import { usePluginUi } from "../render/pluginUiStore";
import { rememberDialogOpener } from "./pluginDialogStore";

export interface PluginMenuEntry {
  plugin: string;
  label: string;
}

/**
 * The user-menu entries plugins registered for the user with `session`, in the order they
 * registered them. A plugin can limit its entry to some users (e.g. bots); none show without a
 * session.
 */
export function usePluginMenuEntries(session: number | undefined): PluginMenuEntry[] {
  const { menuEntries } = usePluginUi();
  if (session === undefined) return [];
  return [...menuEntries]
    .filter(([, entry]) => entry.sessions === null || entry.sessions.includes(session))
    .map(([plugin, entry]) => ({ plugin, label: entry.label }));
}

/** The user picked a plugin's entry for `session`; the plugin answers by opening its dialog. */
export function requestPluginMenu(
  plugin: string,
  session: number,
  opener: HTMLElement | undefined,
): void {
  rememberDialogOpener(plugin, opener);
  getApi()
    .pluginUiEvent(plugin, { type: "userMenuRequested", session })
    .catch((error: unknown) => {
      console.error("Asking a plugin for its user menu failed", toEngineError(error).code);
    });
}
