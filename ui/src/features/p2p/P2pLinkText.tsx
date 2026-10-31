import { useTranslation } from "react-i18next";
import type { P2pLink } from "../../api";
import "./p2p.css";

/**
 * How a P2P connection runs (FR-4.19), e.g. "Via relay (TCP 443) · IP hidden". "IP hidden"
 * carries a tooltip; screen readers get the same sentence as part of the text.
 */
export function P2pLinkText({ link }: { link: P2pLink }) {
  const { t } = useTranslation("p2p");
  const hint = t("link.ipHiddenHint");
  return (
    <span className="p2p-link">
      {t(`link.${link.path}`)}
      {" · "}
      {link.ipHidden ? (
        <span data-tooltip={hint}>
          {t("link.ipHidden")}
          <span className="visually-hidden">{` (${hint})`}</span>
        </span>
      ) : (
        t("link.ipVisible")
      )}
    </span>
  );
}
