/**
 * The plugin dialog (FR-6.40, `dialog` slot): a modal inside the main window that renders the
 * open plugin's tree. Focus stays inside, Escape or "Close" ends it with `dialog-closed`; the
 * plugin then closes it itself (or keeps it, e.g. to save a draft).
 */
import { useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../../a11y";
import { getApi, toEngineError } from "../../../api";
import { Modal } from "../../userpanel/Modal";
import { PluginTree } from "../render/PluginTree";
import { usePluginTree, usePluginUi } from "../render/pluginUiStore";
import { PluginWebView, usePluginWebView } from "../webview";
import { takeDialogOpener } from "./pluginDialogStore";

/** Renders the open plugin dialog, if any; mount once. */
export function PluginDialogHost() {
  const { dialogs } = usePluginUi();
  // One dialog at a time: the plugin that opened last wins.
  const open = [...dialogs.entries()].at(-1);
  return open ? <PluginDialog key={open[0]} plugin={open[0]} title={open[1]} /> : null;
}

function PluginDialog({ plugin, title }: { plugin: string; title: string }) {
  const { t } = useTranslation("pluginDialog");
  const nodes = usePluginTree(plugin, { kind: "dialog" });
  // An open web view replaces the dialog tree (FR-6.46).
  const webview = usePluginWebView(plugin, "dialog");

  const close = useCallback(() => {
    getApi()
      .pluginUiEvent(plugin, { type: "dialogClosed" })
      .catch((error: unknown) => {
        console.error("Closing a plugin dialog failed", toEngineError(error).code);
        announce(t("failed"), "assertive");
      });
  }, [plugin, t]);

  // Focus goes back to whatever opened the dialog once it is gone.
  useEffect(
    () => () => {
      const opener = takeDialogOpener(plugin);
      if (opener?.isConnected) opener.focus();
    },
    [plugin],
  );

  return (
    <Modal title={title} onClose={close}>
      {webview ? (
        <PluginWebView plugin={plugin} webview={webview} title={title} />
      ) : (
        nodes && <PluginTree plugin={plugin} slot={{ kind: "dialog" }} nodes={nodes} />
      )}
      <div className="userpanel-form__actions">
        <button type="button" className="button" onClick={close}>
          {t("close")}
        </button>
      </div>
    </Modal>
  );
}
