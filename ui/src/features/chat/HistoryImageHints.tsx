/**
 * Hints under a synced message whose referenced images are still placeholders (FR-1.60f):
 * "Image not available" when the engine could not load one, and a "Load image" button while
 * "Load full-size images from others" is off.
 */
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { historyImageReferences, loadHistoryImage, useHistoryImageStates } from "./historyImages";

export function HistoryImageHints({
  html,
  serverId,
  tabIndex,
}: {
  html: string;
  serverId: number | null;
  tabIndex: number;
}) {
  const { t } = useTranslation("chat");
  const references = useMemo(() => historyImageReferences(html), [html]);
  const stateOf = useHistoryImageStates();
  const hinted = references.flatMap((reference) => {
    const state = serverId === null ? undefined : stateOf(serverId, reference);
    return state === undefined ? [] : [{ reference, state }];
  });
  if (hinted.length === 0) return null;
  return (
    <>
      {hinted.map(({ reference, state }) => (
        <div key={reference} className="history-image-hint">
          {state === "manual" ? (
            <button
              type="button"
              className="history-image-hint__load"
              tabIndex={tabIndex}
              onClick={() => {
                if (serverId !== null) loadHistoryImage(serverId, reference);
              }}
            >
              {t("historyImage.load")}
            </button>
          ) : (
            <span role="status">{t("historyImage.unavailable")}</span>
          )}
        </div>
      ))}
    </>
  );
}
