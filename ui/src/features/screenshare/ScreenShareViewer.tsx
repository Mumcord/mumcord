/**
 * A watched screen share in a floating widget over the app (FR-2.45, FR-2.70, spec 5.4); the
 * chat stays usable next to it. It collapses to a slim bar (sharer name + "Live"), goes full
 * screen via button or F (Escape leaves it) and "Stop watching" closes it.
 */
import { useEffect, useRef } from "react";
import { FloatingWidget } from "../../components/FloatingWidget";
import type { Rect, Size } from "../../components/floatingWidgetGeometry";
import { useTranslation } from "react-i18next";
import { useServerStateOf } from "../channels";
import { P2pLinkText } from "../p2p/P2pLinkText";
import { useP2pLink } from "../p2p/useP2pLink";
import { ScreenStage } from "./ScreenStage";
import { useScreenShareOf } from "./sharesStore";
import { useFullscreen } from "../media/useFullscreen";
import { stopWatching, useWatchedShare, type WatchedShare } from "./watching";
import "./screenViewer.css";

/** Opens top right over the chat, clear of the member column. */
function initialRect(window: Size): Rect {
  const width = 480;
  return { x: window.width - width - 280, y: 64, width, height: width / (16 / 9) + 40 };
}

/** The watched share, if any; mounted at app-shell level so it survives switching chats. */
export function ScreenShareWidget() {
  const share = useWatchedShare();
  if (share === null) return null;
  return <ScreenShareViewer share={share} />;
}

export interface ScreenShareViewerProps {
  share: WatchedShare;
}

function typingIn(target: EventTarget): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName))
  );
}

export function ScreenShareViewer({ share }: ScreenShareViewerProps) {
  const { t } = useTranslation("screenviewer");
  const { serverId, session } = share;
  const paused = useScreenShareOf(serverId, session)?.paused ?? false;
  const name = useServerStateOf(serverId).users.get(session)?.name ?? t("someone");
  const link = useP2pLink(serverId, session);
  const viewerRef = useRef<HTMLElement>(null);
  const { fullscreen, toggle, exit } = useFullscreen(viewerRef);

  // F works anywhere while the viewer is open, except while typing (e.g. in the chat above it).
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey) return;
      if (event.target !== null && typingIn(event.target)) return;
      if (event.key === "f" || event.key === "F") {
        event.preventDefault();
        toggle();
      } else if (event.key === "Escape" && fullscreen) {
        event.preventDefault();
        exit();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [toggle, exit, fullscreen]);

  return (
    <FloatingWidget
      ref={viewerRef}
      kind="screenShare"
      className="screen-viewer"
      label={t("label", { name })}
      title={
        <>
          {t("title", { name })}
          <span className="screen-viewer__beta">{t("beta")}</span>
          {link && (
            <span className="screen-viewer__link">
              <P2pLinkText link={link} />
            </span>
          )}
        </>
      }
      collapsedTitle={
        <>
          {name}
          <span className="screen-viewer__live">{t("live")}</span>
        </>
      }
      actions={
        <button
          type="button"
          className="button"
          aria-label={fullscreen ? t("exitFullscreen") : t("fullscreen")}
          data-tooltip={fullscreen ? t("exitFullscreen") : t("fullscreen")}
          aria-pressed={fullscreen}
          aria-keyshortcuts="F"
          onClick={toggle}
        >
          <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" focusable="false">
            <path
              d={
                fullscreen
                  ? "M6 2v4H2M10 2v4h4M6 14v-4H2M10 14v-4h4"
                  : "M2 6V2h4M14 6V2h-4M2 10v4h4M14 10v4h-4"
              }
              fill="none"
              stroke="currentColor"
              strokeWidth="1.6"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        </button>
      }
      close={{ label: t("stop"), onClose: stopWatching }}
      aspect={16 / 9}
      initialRect={initialRect}
    >
      <ScreenStage
        key={`${String(serverId)}:${String(session)}`}
        serverId={serverId}
        session={session}
        name={name}
        paused={paused}
      />
    </FloatingWidget>
  );
}
