/** The "Drop to share" cover over the channel chat while files are dragged over it (mockup 1a). */
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import "./composer.css";

export function FileDropOverlay({ host, channel }: { host: HTMLElement; channel: string }) {
  const { t } = useTranslation("files");
  return createPortal(
    <div className="file-drop" aria-hidden="true">
      <div className="file-drop__box">
        <span className="file-drop__icon">
          <svg
            viewBox="0 0 24 24"
            width="32"
            height="32"
            aria-hidden="true"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M12 21V9M7 14l5-5 5 5M5 3h14" />
          </svg>
        </span>
        <p className="file-drop__title">{t("composer.dropTitle", { channel })}</p>
        <p className="file-drop__hint">{t("composer.dropHint")}</p>
      </div>
    </div>,
    host,
  );
}
