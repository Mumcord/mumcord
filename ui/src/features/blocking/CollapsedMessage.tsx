import { useTranslation } from "react-i18next";
import { BlockIcon } from "./BlockIcon";
import "./blocking.css";

/** Stands in for a channel message of a blocked user; Show reveals that one message. */
export function CollapsedMessage({ onShow, tabIndex }: { onShow: () => void; tabIndex: number }) {
  const { t } = useTranslation("members");
  return (
    <div className="blocked-message" data-testid="blocked-message">
      <BlockIcon className="blocked-message__icon" size={18} />
      <span className="blocked-message__text">{t("block.collapsed")}</span>
      <button
        type="button"
        className="blocked-message__show"
        tabIndex={tabIndex}
        aria-label={t("block.showLabel")}
        onClick={onShow}
      >
        {t("block.show")}
      </button>
    </div>
  );
}
