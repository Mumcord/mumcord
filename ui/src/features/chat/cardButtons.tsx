/**
 * Buttons shared by the cards under a message: link preview, player and inline image cards.
 */
import { useTranslation } from "react-i18next";
import { MediaIcon, type MediaIconName } from "../media/icons";
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
      data-tooltip={t("preview.dismiss")}
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

interface ControlButtonProps {
  label: string;
  icon: MediaIconName;
  onClick: () => void;
  tabIndex: number;
  /** Set when the button is a toggle that is on, such as full screen. */
  pressed?: boolean;
  keyShortcuts?: string;
  /** The bar's main control, the play button: the larger glass pill of the overlay. */
  main?: boolean;
  /** A hint under the label when the button does something other than its name says. */
  tooltip?: string | undefined;
}

/**
 * One button of a card's player toolbar (issue #419). The toolbar sits inside the element that
 * goes full screen, so it can still be clicked there: full screen puts the player on the screen,
 * and the same button takes it back. It is the shared media overlay icon, so a card reads like
 * the watch party (issue #163).
 */
export function ControlButton({
  label,
  icon,
  onClick,
  tabIndex,
  pressed,
  keyShortcuts,
  main = false,
  tooltip,
}: ControlButtonProps) {
  return (
    <button
      type="button"
      className={`button media-overlay__icon${main ? " media-overlay__icon--main" : ""}`}
      tabIndex={tabIndex}
      aria-label={label}
      data-tooltip={tooltip ?? label}
      aria-pressed={pressed}
      aria-keyshortcuts={keyShortcuts}
      onClick={onClick}
    >
      <MediaIcon name={icon} />
    </button>
  );
}
