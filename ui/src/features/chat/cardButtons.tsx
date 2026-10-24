/**
 * Buttons shared by the cards under a message: link preview, player and inline image cards.
 */
import { useTranslation } from "react-i18next";
import { openInBrowser } from "./openLink";

export function DismissButton({
  onDismiss,
  tabIndex,
}: {
  onDismiss: () => void;
  tabIndex: number;
}) {
  const { t } = useTranslation("chat");
  return (
    <button
      type="button"
      className="link-card__dismiss"
      tabIndex={tabIndex}
      aria-label={t("preview.dismiss")}
      title={t("preview.dismiss")}
      onClick={onDismiss}
    >
      <span aria-hidden="true">×</span>
    </button>
  );
}

interface OpenButtonProps {
  url: string;
  title: string;
  tabIndex: number;
  className?: string;
}

export function OpenButton({
  url,
  title,
  tabIndex,
  className = "link-card__open",
}: OpenButtonProps) {
  const { t } = useTranslation("chat");
  return (
    <button
      type="button"
      className={className}
      tabIndex={tabIndex}
      aria-label={t("preview.openLabel", { title })}
      onClick={() => {
        openInBrowser(url);
      }}
    >
      {t("preview.open")}
    </button>
  );
}
