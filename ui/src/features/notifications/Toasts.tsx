import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import type { Conversation } from "../../api";
import { markSeen, removeToast, useToasts, type Toast } from "./notificationStore";

/** How long a toast stays without hover or focus. */
export const TOAST_MS = 6000;

interface ToastsProps {
  onOpen: (serverId: number, conversation: Conversation) => void;
}

/** Closing a focused toast must not drop keyboard focus onto the page body. */
function focusChatRegion(): void {
  document.querySelector<HTMLElement>("#chat")?.focus();
}

function ToastItem({
  toast,
  paused,
  onOpen,
}: {
  toast: Toast;
  paused: boolean;
  onOpen: (serverId: number, conversation: Conversation) => void;
}) {
  const { t } = useTranslation("notifications");

  useEffect(() => {
    if (paused) return;
    const timer = setTimeout(() => {
      removeToast(toast.key);
    }, TOAST_MS);
    return () => {
      clearTimeout(timer);
    };
  }, [toast.key, toast.version, paused]);

  function close(): void {
    markSeen(toast.key);
    focusChatRegion();
  }

  function closeOnEscape(event: KeyboardEvent): void {
    if (event.key !== "Escape") return;
    event.stopPropagation();
    close();
  }

  return (
    <div className="toast">
      <button
        type="button"
        className="toast__open"
        onKeyDown={closeOnEscape}
        onClick={() => {
          onOpen(toast.serverId, toast.conversation);
        }}
      >
        <span className="toast__title">{toast.title}</span>
        {toast.text && <span className="toast__text">{toast.text}</span>}
      </button>
      <button
        type="button"
        className="toast__close"
        aria-label={t("close")}
        onKeyDown={closeOnEscape}
        onClick={close}
      >
        <span aria-hidden="true">×</span>
      </button>
    </div>
  );
}

/** Toast stack bottom right; timers pause while the pointer or keyboard focus is on it. */
export function Toasts({ onOpen }: ToastsProps) {
  const { t } = useTranslation("notifications");
  const toasts = useToasts();
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const region = useRef<HTMLElement>(null);

  // Native listeners: the region is a landmark, not a control, so it takes no JSX handlers.
  useEffect(() => {
    const element = region.current;
    if (!element) return;
    const enter = () => {
      setHovered(true);
    };
    const leave = () => {
      setHovered(false);
    };
    const focusIn = () => {
      setFocused(true);
    };
    const focusOut = (event: FocusEvent) => {
      if (!(event.relatedTarget instanceof Node && element.contains(event.relatedTarget))) {
        setFocused(false);
      }
    };
    element.addEventListener("mouseenter", enter);
    element.addEventListener("mouseleave", leave);
    element.addEventListener("focusin", focusIn);
    element.addEventListener("focusout", focusOut);
    return () => {
      element.removeEventListener("mouseenter", enter);
      element.removeEventListener("mouseleave", leave);
      element.removeEventListener("focusin", focusIn);
      element.removeEventListener("focusout", focusOut);
    };
  }, []);

  return (
    <section ref={region} className="toasts" aria-label={t("region")}>
      {/* Present before any toast arrives so screen readers pick up additions. */}
      <div className="toasts__list" role="status" aria-live="polite">
        {toasts.map((toast) => (
          <ToastItem key={toast.key} toast={toast} paused={hovered || focused} onOpen={onOpen} />
        ))}
      </div>
    </section>
  );
}
