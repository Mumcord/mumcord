import { getApi, toEngineError } from "../../../api";
import { usePluginUi } from "../render/pluginUiStore";
import { rememberDialogOpener } from "./pluginDialogStore";

export interface PluginMenuEntry {
  plugin: string;
  label: string;
}

/** The user-menu entries plugins registered, in the order they registered them. */
export function usePluginMenuEntries(): PluginMenuEntry[] {
  const { menuEntries } = usePluginUi();
  return [...menuEntries].map(([plugin, label]) => ({ plugin, label }));
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
