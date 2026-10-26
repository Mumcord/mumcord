import { useEffect, useId, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useThemePreviewing } from "../theme/themeStore";
import { SettingsMount } from "./mounts";

/**
 * Full-window modal for the settings screen. The shell behind it is made inert by the caller;
 * Escape closes it and focus returns to the element that opened it.
 */
export function SettingsOverlay({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation(["app", "common"]);
  const titleId = useId();
  const dialogRef = useRef<HTMLDivElement>(null);
  const previewing = useThemePreviewing();
  // Escape ends the preview, not Settings; read through a ref so toggling it never re-runs the effect.
  const previewingRef = useRef(previewing);
  useEffect(() => {
    previewingRef.current = previewing;
  }, [previewing]);

  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialogRef.current?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !previewingRef.current) onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      opener?.focus();
    };
  }, [onClose]);

  // Hidden, not unmounted, so the settings state and the editor draft survive the preview.
  return (
    <div
      className="overlay"
      hidden={previewing}
      inert={previewing}
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
