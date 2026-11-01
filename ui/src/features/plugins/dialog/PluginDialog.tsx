/**
 * The plugin dialog (FR-6.40, `dialog` slot): a modal inside the main window that renders the
 * open plugin's tree under a host-drawn header (plugin icon, title, subtitle, ✕). Focus stays
 * inside, Escape or ✕ ends it at once on the host side and then tells the plugin with
 * `dialog-closed`; the user is always in control.
 */
import { useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../../a11y";
import { getApi, toEngineError } from "../../../api";
import { Modal } from "../../userpanel/Modal";
import { PluginTree } from "../render/PluginTree";
import { usePluginIcon } from "../render/usePluginIcon";
import {
  closePluginDialog,
  usePluginTree,
  usePluginUi,
  type PluginDialogHeader,
} from "../render/pluginUiStore";
import { PluginWebView, usePluginWebView } from "../webview";
import { takeDialogOpener } from "./pluginDialogStore";

/** Renders the open plugin dialog, if any; mount once. */
export function PluginDialogHost() {
  const { dialogs } = usePluginUi();
  // One dialog at a time: the plugin that opened last wins.
  const open = [...dialogs.entries()].at(-1);
  return open ? <PluginDialog key={open[0]} plugin={open[0]} header={open[1]} /> : null;
}

function PluginDialog({ plugin, header }: { plugin: string; header: PluginDialogHeader }) {
  const { t } = useTranslation("pluginDialog");
  const { title, subtitle } = header;
  const icon = usePluginIcon(plugin);
  const nodes = usePluginTree(plugin, { kind: "dialog" });
  // An open web view replaces the dialog tree (FR-6.46).
  const webview = usePluginWebView(plugin, "dialog");

  const close = useCallback(() => {
    // Closed here first: the plugin only gets told, it cannot keep the dialog open.
    closePluginDialog(plugin);
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
    <Modal
      title={title}
      subtitle={subtitle}
      icon={icon === null ? undefined : <img src={icon} alt="" />}
      closeLabel={t("close")}
      onClose={close}
    >
      {webview ? (
        <PluginWebView plugin={plugin} webview={webview} title={title} />
      ) : (
        nodes && <PluginTree plugin={plugin} slot={{ kind: "dialog" }} nodes={nodes} />
      )}
    </Modal>
  );
}
