import { useTranslation } from "react-i18next";
import { gifSource } from "./gifSource";
import { openInBrowser } from "./openLink";

/** Small "via GIPHY" / "via Tenor" credit linking to the original page (QA.52). */
export function GifAttribution({
  url,
  className,
  tabIndex,
}: {
  /** The GIF's link; the credit is hidden for any other source. */
  url: string;
  className: string;
  tabIndex?: number;
}) {
  const { t } = useTranslation("chat");
  const source = gifSource(url);
  if (!source) return null;
  const brand = source.brand === "giphy" ? "GIPHY" : "Tenor";
  return (
    <a
      className={className}
      href={source.href}
      tabIndex={tabIndex}
      data-tooltip={t("attribution.view", { brand })}
      onClick={(event) => {
        event.preventDefault();
        openInBrowser(source.href);
      }}
    >
      {t("attribution.via", { brand })}
    </a>
  );
}
