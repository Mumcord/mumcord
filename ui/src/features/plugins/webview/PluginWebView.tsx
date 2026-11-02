/**
 * A plugin's sealed web view (FR-6.46 … FR-6.48): an official plugin's page from its `web/`
 * folder in a frame with an opaque origin. `sandbox="allow-scripts"` without
 * `allow-same-origin` keeps it away from storage, the app window and Tauri; the page's own
 * Content-Security-Policy (set by the `mumcord-web` protocol) blocks the network. The frame may
 * load its page once: any further load is a navigation, and the frame is removed.
 *
 * Keyboard: Tab moves into and out of the frame as usual; Escape inside the page moves focus to
 * the frame's wrapper, so the next Escape reaches the dialog or popover around it.
 */
import { useEffect, useRef, useState } from "react";
import { getApi, toEngineError } from "../../../api";
import type { PluginWebview } from "../render/pluginUiStore";
import { createBridge, type Bridge } from "./bridge";
import "./webview.css";

interface Props {
  readonly plugin: string;
  readonly webview: PluginWebview;
  /** The accessible name of the frame, e.g. the dialog title or the plugin's name. */
  readonly title: string;
}

/** Renders an open web view; the caller decides where (see {@link usePluginWebView}). */
export function PluginWebView({ plugin, webview, title }: Props) {
  // A new `seq` (the plugin showed a page again) gets a fresh frame.
  return <SealedFrame key={webview.seq} plugin={plugin} url={webview.url} title={title} />;
}

function SealedFrame({ plugin, url, title }: { plugin: string; url: string; title: string }) {
  const wrapper = useRef<HTMLDivElement>(null);
  const frame = useRef<HTMLIFrameElement>(null);
  const loads = useRef(0);
  const [navigated, setNavigated] = useState(false);

  const bridge = useRef<Bridge | null>(null);

  useEffect(() => {
    let warned = false;
    const current = createBridge({
      frame: () => frame.current?.contentWindow ?? null,
      toPlugin: (text) => {
        getApi()
          .pluginUiEvent(plugin, { type: "webviewMessage", text })
          .catch((error: unknown) => {
            // Dropped by the limits or the plugin stopped; the text is never logged.
            console.debug("A web view message was not delivered", toEngineError(error).code);
          });
      },
      onEscape: () => wrapper.current?.focus(),
      onDropped: () => {
        if (warned) return;
        warned = true;
        console.warn("A plugin web view sent messages above the bridge limits; they were dropped");
      },
    });
    bridge.current = current;
    if (loads.current > 0) current.loaded();
    window.addEventListener("message", current.receive);
    const off = getApi().on("pluginUi", (event) => {
      if (event.plugin === plugin && event.change.type === "webviewPost") {
        current.post(event.change.message);
      }
    });
    return () => {
      window.removeEventListener("message", current.receive);
      off();
      bridge.current = null;
    };
  }, [plugin]);

  const onLoad = () => {
    loads.current += 1;
    if (loads.current === 1) {
      bridge.current?.loaded();
      return;
    }
    // The page navigated away from its own file: never keep what it loaded.
    console.warn("A plugin web view navigated and was closed");
    setNavigated(true);
  };

  return (
    <div ref={wrapper} className="plugin-webview" tabIndex={-1}>
      {!navigated && (
        <iframe
          ref={frame}
          className="plugin-webview__frame"
          src={url}
          title={title}
          sandbox="allow-scripts"
          referrerPolicy="no-referrer"
          allow=""
          onLoad={onLoad}
        />
      )}
    </div>
  );
}
