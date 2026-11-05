/**
 * A plugin's web content in a modal Mumcord dialog (FR-9.40, mockup 05): header with the plugin's
 * icon, name, an "Unofficial" chip and ✕, the frame, and the footer reminder (FR-9.36). Esc or ✕
 * closes it on the host side first and then tells the plugin with `surface-closed`.
 *
 * It is portalled to `document.body` so it never sits inside a form (nested forms broke before).
 * Tab stays inside the dialog: sentinels around the content send focus on from the frame, whose
 * own Tab handling Mumcord cannot see; Esc/F6 inside the page already return focus to the dialog.
 */
import { useCallback, useEffect, useId, useRef, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { announce } from "../../../a11y";
import { getApi, toEngineError } from "../../../api";
import { takeDialogOpener } from "../dialog/pluginDialogStore";
import { usePlugins } from "../pluginStore";
import { closePluginDialog, usePluginUi, type PluginWebview } from "../render/pluginUiStore";
import { PluginTile } from "../render/PluginTile";
import { usePluginIcon } from "../render/usePluginIcon";
import { PluginFrame } from "../webview";
import "./webdialog.css";

interface Props {
  readonly plugin: string;
  readonly title: string;
  readonly webview: PluginWebview;
}

const FOCUSABLE = 'button, iframe, [tabindex]:not([tabindex="-1"])';

/** The open web dialog of `plugin`; the caller mounts it only while a dialog web view is open. */
export function WebDialog({ plugin, title, webview }: Props) {
  const { t } = useTranslation("pluginStage");
  const { t: tDialog } = useTranslation("pluginDialog");
  const titleId = useId();
  const box = useRef<HTMLDivElement>(null);
  const installed = usePlugins().overview?.plugins.find((p) => p.id === plugin);
  const name = installed?.name || plugin;
  const icon = usePluginIcon(plugin);
  const hint = usePluginUi().sizeHints?.get(plugin);
  const source = installed?.source;
  const chip =
    source === undefined || source.kind === "official"
      ? null
      : source.kind === "repo" && source.name !== ""
        ? t("unofficialRepository", { repository: source.name })
        : t("unofficial");

  const close = useCallback(() => {
    // Closed here first: the plugin only gets told, it cannot keep the dialog open.
    closePluginDialog(plugin);
    getApi()
      .pluginUiEvent(plugin, { type: "surfaceClosed", place: "dialog" })
      .catch((error: unknown) => {
        console.error("Closing a plugin web dialog failed", toEngineError(error).code);
        announce(tDialog("failed"), "assertive");
      });
  }, [plugin, tDialog]);

  // Focus starts on the content and goes back to whatever opened the dialog.
  useEffect(() => {
    box.current?.querySelector<HTMLElement>(".plugin-webview")?.focus();
    return () => {
      const opener = takeDialogOpener(plugin);
      if (opener?.isConnected) opener.focus();
    };
  }, [plugin]);

  useEffect(() => {
    const dialog = box.current;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      close();
    };
    dialog?.addEventListener("keydown", onKeyDown);
    return () => {
      dialog?.removeEventListener("keydown", onKeyDown);
    };
  }, [close]);

  const focusEnd = (last: boolean) => {
    const items = [...(box.current?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? [])];
    (last ? items.at(-1) : items[0])?.focus();
  };

  // The page asks for more room than the default; the dialog stays within 46 × 34 rem (FR-9.35).
  const style = hint
    ? ({ "--plugin-dialog-min-height": `${String(hint.minHeight)}px` } as CSSProperties)
    : undefined;

  return createPortal(
    <div className="settings-dialog__backdrop">
      <span
        role="presentation"
        // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- focus sentinel, see header
        tabIndex={0}
        data-focus-sentinel=""
        onFocus={() => {
          focusEnd(true);
        }}
      />
      <div
        ref={box}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className="settings-dialog plugin-web-dialog"
        style={style}
      >
        <div className="plugin-web-dialog__head">
          {icon === null ? (
            <PluginTile id={plugin} name={name} className="plugin-web-dialog__icon" />
          ) : (
            <img className="plugin-web-dialog__icon" src={icon} alt="" />
          )}
          <h3 id={titleId} className="settings-dialog__title settings-dialog__title--tight">
            {title || name}
          </h3>
          {chip !== null && <span className="plugin-web-dialog__chip">{chip}</span>}
          <button
            type="button"
            className="plugin-web-dialog__close"
            aria-label={t("dialog.close", { plugin: name })}
            data-tooltip={t("dialog.close", { plugin: name })}
            onClick={close}
          >
            <svg
              viewBox="0 0 24 24"
              width="16"
              height="16"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
              aria-hidden="true"
            >
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>
        </div>
        <div className="plugin-web-dialog__body">
          <PluginFrame plugin={plugin} webview={webview} onEscape={close} />
        </div>
        <p className="settings-hint plugin-web-dialog__footer">{t("dialog.footer")}</p>
      </div>
      <span
        role="presentation"
        // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- focus sentinel, see header
        tabIndex={0}
        data-focus-sentinel=""
        onFocus={() => {
          focusEnd(false);
        }}
      />
    </div>,
    document.body,
  );
}
