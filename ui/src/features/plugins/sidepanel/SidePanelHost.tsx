/**
 * The plugin side panel (FR-9.39, mockup 04): web content docked right of the chat, in the place
 * of the member list. One panel per app: the engine closes the first plugin's panel when another
 * opens (and tells it `surface-closed`); should two ever be open here, the newest wins. Closing it
 * with the ✕ hides it at once and tells the plugin with `surface-closed` (`place: sidePanel`).
 *
 * The shell's member column stays mounted underneath (hidden) so its state survives. Its width
 * follows the panel through the shell's own column variables (see sidepanel.css).
 */
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../../a11y";
import { getApi, toEngineError } from "../../../api";
import { useMembersColumn } from "../../../app/membersCollapse";
import { ResizeHandle } from "../../../app/ResizeHandle";
import { usePlugins } from "../pluginStore";
import { usePluginIcon } from "../render/usePluginIcon";
import { usePluginUi, type PluginWebview } from "../render/pluginUiStore";
import { PluginFrame } from "../webview";
import {
  clampSideWidth,
  dismissSidePanel,
  loadSideWidth,
  minWidthForHint,
  saveSideWidth,
  SIDE_PANEL_DEFAULT_REM,
  SIDE_PANEL_MAX_REM,
  SIDE_PANEL_STEP_REM,
  useDismissedSidePanels,
} from "./sidePanelStore";
import "./sidepanel.css";

interface Open {
  readonly plugin: string;
  readonly webview: PluginWebview;
}

function useOpenSidePanel(): Open | undefined {
  const { webviews } = usePluginUi();
  const dismissed = useDismissedSidePanels();
  let newest: Open | undefined;
  for (const [plugin, places] of webviews) {
    const webview = places.get("sidePanel");
    if (webview === undefined || dismissed.get(plugin) === webview.seq) continue;
    if (newest === undefined || webview.seq > newest.webview.seq) newest = { plugin, webview };
  }
  return newest;
}

/** Shows the open side panel instead of `children` (the member list). */
export function SidePanelHost({ children }: { readonly children: ReactNode }) {
  const open = useOpenSidePanel();
  return (
    <>
      <div hidden={open !== undefined} className="plugin-sidepanel__members">
        {children}
      </div>
      {open && <SidePanel key={open.plugin} open={open} />}
    </>
  );
}

function SidePanel({ open }: { readonly open: Open }) {
  const { plugin, webview } = open;
  const { t } = useTranslation("pluginSidePanel");
  const { t: tStage } = useTranslation("pluginStage");
  const installed = usePlugins().overview?.plugins.find((p) => p.id === plugin);
  const name = installed?.name || plugin;
  const unofficial = installed !== undefined && installed.source.kind !== "official";
  const icon = usePluginIcon(plugin);
  const hint = usePluginUi().sizeHints?.get(plugin);
  const column = useMembersColumn();

  const min = minWidthForHint(hint?.minWidth);
  const [chosen, setChosen] = useState(() => loadSideWidth(plugin) ?? SIDE_PANEL_DEFAULT_REM);
  const width = Math.max(chosen, min);
  const root = useRef<HTMLElement>(null);

  // The member column must show for the panel to be seen; it goes back as it was on close.
  const columnRef = useRef(column);
  useEffect(() => {
    columnRef.current = column;
  });
  useEffect(() => {
    const opened = columnRef.current;
    const reopened = opened !== null && !opened.shown;
    if (reopened) opened.toggle();
    return () => {
      const now = columnRef.current;
      if (reopened && now?.shown) now.toggle();
    };
  }, []);

  // The grid column is the shell's; it takes the panel's width through its own variables.
  useLayoutEffect(() => {
    const shell = root.current?.closest<HTMLElement>(".shell");
    if (!shell) return;
    shell.dataset.pluginSidePanel = "true";
    shell.style.setProperty("--plugin-side-width", `${String(width)}rem`);
    return () => {
      delete shell.dataset.pluginSidePanel;
      shell.style.removeProperty("--plugin-side-width");
    };
  }, [width]);

  // Focus goes back to what had it before the panel opened (the opener may be gone: the chat).
  const opener = useRef(document.activeElement);
  useEffect(() => {
    const from = opener.current;
    return () => {
      if (from instanceof HTMLElement && from.isConnected) from.focus();
      else columnRef.current?.buttonRef.current?.focus();
    };
  }, []);

  const close = useCallback(() => {
    dismissSidePanel(plugin, webview.seq);
    getApi()
      .pluginUiEvent(plugin, { type: "surfaceClosed", place: "sidePanel" })
      .catch((error: unknown) => {
        console.error("Closing a plugin side panel failed", toEngineError(error).code);
        announce(t("failed", { plugin: name }), "assertive");
      });
  }, [plugin, webview.seq, name, t]);

  const resizeTo = (rem: number) => {
    const next = clampSideWidth(rem);
    setChosen(next);
    return next;
  };

  const id = `plugin-sidepanel-${plugin}`;
  return (
    <section
      ref={root}
      id={id}
      className="plugin-sidepanel"
      aria-label={t("region", { plugin: name })}
    >
      <ResizeHandle
        name="plugin-sidepanel"
        label={t("resize", { plugin: name })}
        controls={id}
        aria={{ now: width, min, max: SIDE_PANEL_MAX_REM, text: `${String(width)} rem` }}
        start={width}
        onDrag={(start, deltaPx, _alt, done) => {
          const remPx = parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
          // Dragging left widens the panel.
          const next = resizeTo(start - deltaPx / remPx);
          if (done) saveSideWidth(plugin, next);
        }}
        onKey={(key) => {
          const target =
            key === "ArrowLeft"
              ? width + SIDE_PANEL_STEP_REM
              : key === "ArrowRight"
                ? width - SIDE_PANEL_STEP_REM
                : key === "Home"
                  ? min
                  : key === "End"
                    ? SIDE_PANEL_MAX_REM
                    : null;
          if (target === null) return false;
          saveSideWidth(plugin, resizeTo(target));
          return true;
        }}
        onReset={() => {
          saveSideWidth(plugin, resizeTo(SIDE_PANEL_DEFAULT_REM));
        }}
      />
      <header className="plugin-sidepanel__head">
        {icon === null ? (
          <span className="plugin-sidepanel__icon" aria-hidden="true">
            {name.slice(0, 1).toUpperCase()}
          </span>
        ) : (
          <img className="plugin-sidepanel__icon" src={icon} alt="" />
        )}
        <h2 className="plugin-sidepanel__name">{name}</h2>
        {unofficial && <span className="plugin-sidepanel__chip">{tStage("unofficial")}</span>}
        <button
          type="button"
          className="plugin-sidepanel__close"
          aria-label={t("close", { plugin: name })}
          data-tooltip={t("close", { plugin: name })}
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
      </header>
      <div className="plugin-sidepanel__body">
        <PluginFrame plugin={plugin} webview={webview} />
      </div>
    </section>
  );
}
