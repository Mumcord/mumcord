import { useEffect, useId, useRef, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { useIsMacos } from "../../api/useIsMacos";
import { Clippy } from "./Clippy";
import { closeTour, nextTip, previousTip, startTour, TIPS, useTour, type TipId } from "./tour";
import "./tips.css";

/**
 * Clippy in the lower right corner of the main window with a speech bubble: the first-start
 * offer, then one tip at a time (#637). Not modal: the app stays usable while it is open.
 */
export function TipsHost() {
  const { t } = useTranslation(["tips", "channels"]);
  const tour = useTour();
  const macos = useIsMacos();
  const titleId = useId();
  const bodyId = useId();
  const primary = useRef<HTMLButtonElement>(null);

  // The offer pops up on its own after the wizard, so screen readers hear it too.
  useEffect(() => {
    if (tour.kind === "offer") announce(`${t("greeting.title")} ${t("greeting.body")}`);
  }, [tour.kind, t]);

  // Keyboard users land on the bubble's main button for every new step.
  useEffect(() => {
    if (tour.kind !== "hidden") primary.current?.focus();
  }, [tour]);

  if (tour.kind === "hidden") return null;

  // Every control of the bubble closes it on Escape; focus is always on one of them.
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key !== "Escape") return;
    event.stopPropagation();
    closeTour();
  };

  let title: string;
  let body: string;
  let progress: string | null = null;
  let actions;
  if (tour.kind === "offer") {
    title = t("greeting.title");
    body = t("greeting.body");
    actions = (
      <>
        <button type="button" className="button" onKeyDown={onKeyDown} onClick={closeTour}>
          {t("greeting.later")}
        </button>
        <button
          ref={primary}
          type="button"
          className="button button--primary"
          onKeyDown={onKeyDown}
          onClick={startTour}
        >
          {t("greeting.start")}
        </button>
      </>
    );
  } else {
    const id: TipId = TIPS[tour.index] ?? "end";
    const last = tour.index === TIPS.length - 1;
    const searchKey = macos ? "⌘K" : t("channels:search.shortcut");
    title = t(`tips.${id}.title`);
    body = t(`tips.${id}.body`, { key: searchKey });
    progress = t("progress", { current: tour.index + 1, total: TIPS.length });
    actions = (
      <>
        {tour.index > 0 && (
          <button type="button" className="button" onKeyDown={onKeyDown} onClick={previousTip}>
            {t("back")}
          </button>
        )}
        <button
          ref={primary}
          type="button"
          className="button button--primary"
          onKeyDown={onKeyDown}
          onClick={nextTip}
        >
          {last ? t("done") : t("next")}
        </button>
      </>
    );
  }

  return (
    <div
      className="tips"
      role="dialog"
      aria-label={t("label")}
      aria-describedby={`${titleId} ${bodyId}`}
    >
      <div className="tips__bubble">
        <button
          type="button"
          className="tips__close"
          aria-label={t("close")}
          onKeyDown={onKeyDown}
          onClick={closeTour}
        >
          <span aria-hidden="true">×</span>
        </button>
        {progress !== null && <p className="tips__progress">{progress}</p>}
        <h2 id={titleId} className="tips__title">
          {title}
        </h2>
        <p id={bodyId} className="tips__body">
          {body}
        </p>
        <div className="tips__actions">{actions}</div>
      </div>
      <Clippy />
    </div>
  );
}
