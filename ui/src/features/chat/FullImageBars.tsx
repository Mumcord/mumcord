/**
 * Slim progress bars under a message while the full-size version of its images loads
 * (FR-2.22). The viewer starts the transfer; the bar tells the chat about it too.
 */
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { FULL_ATTRIBUTE, FULL_REFERENCE, cancelFullImage, useLoadingFullImages } from "./fullImage";

const REFERENCE_ATTRIBUTE = new RegExp(`${FULL_ATTRIBUTE}="([^"]*)"`, "g");

/** The full-image references the message's HTML names, once each. */
function referencesIn(html: string): string[] {
  const found = new Set<string>();
  for (const match of html.matchAll(REFERENCE_ATTRIBUTE)) {
    const reference = match[1];
    if (reference !== undefined && FULL_REFERENCE.test(reference)) found.add(reference);
  }
  return [...found];
}

export function FullImageBars({ html }: { html: string }) {
  const { t } = useTranslation("chat");
  const references = useMemo(() => referencesIn(html), [html]);
  const loading = useLoadingFullImages();
  const active = references.filter((reference) => loading.has(reference));
  if (active.length === 0) return null;
  return (
    <>
      {active.map((reference) => {
        const progress = loading.get(reference) ?? null;
        const percent = progress === null ? null : Math.floor(progress * 100);
        return (
          <div key={reference} className="full-image-transfer">
            <div
              className="full-image-bar"
              role="progressbar"
              aria-label={t("viewer.fullLoading")}
              aria-valuemin={0}
              aria-valuemax={100}
              {...(percent === null ? {} : { "aria-valuenow": percent })}
            >
              <div className="full-image-bar__fill" style={{ width: `${String(percent ?? 0)}%` }} />
            </div>
            <button
              type="button"
              className="full-image-transfer__cancel"
              aria-label={t("viewer.fullCancel")}
              data-tooltip={t("viewer.fullCancel")}
              onClick={() => {
                cancelFullImage(reference);
              }}
            >
              ×
            </button>
          </div>
        );
      })}
    </>
  );
}
