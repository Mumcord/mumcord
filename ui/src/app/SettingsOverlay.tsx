import { useEffect, useId, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useThemePreviewing } from "../theme/themeStore";
import { SettingsMount } from "./mounts";

/**
 * Full-window modal for the settings screen. The shell behind it is made inert by the caller;
 * Escape closes it and focus returns to the element that opened it. `blocked` hands Escape and
 * the focus ring to a modal on top (the wizard rerun, #149) while keeping the settings mounted.
 */
export function SettingsOverlay({
  onClose,
  blocked = false,
}: {
  onClose: () => void;
  blocked?: boolean;
}) {
  const { t } = useTranslation(["app", "common"]);
  const titleId = useId();
  const dialogRef = useRef<HTMLDivElement>(null);
  const previewing = useThemePreviewing();
  // Escape ends the preview, not Settings; read through a ref so toggling it never re-runs the effect.
  const previewingRef = useRef(previewing);
  useEffect(() => {
    previewingRef.current = previewing;
  }, [previewing]);

  // Focus moves in once and returns to the opener on unmount, whatever blocks the dialog later.
  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialogRef.current?.focus();
    return () => {
      opener?.focus();
    };
  }, []);

  useEffect(() => {
    if (blocked) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !previewingRef.current) onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [onClose, blocked]);

  // Hidden, not unmounted, so the settings state and the editor draft survive the preview.
  return (
    <div
      className="overlay"
      hidden={previewing}
      inert={previewing || blocked}
      style={previewing ? { display: "none" } : undefined}
    >
      <div
        ref={dialogRef}
        className="overlay__dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
      >
        <header className="overlay__header">
          <h1 id={titleId}>{t("settings.title")}</h1>
          <button type="button" className="overlay__close" onClick={onClose}>
            {t("common:close")}
          </button>
        </header>
        <div className="overlay__body">
          <SettingsMount />
        </div>
      </div>
    </div>
  );
}
