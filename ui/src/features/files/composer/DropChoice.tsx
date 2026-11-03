/**
 * Asked when a drop into a channel holds images: send them inline (today's path) or share them as
 * files. Enter sends as image, Escape cancels.
 */
import { useEffect, useId, useRef } from "react";
import { useTranslation } from "react-i18next";
import "./composer.css";

export function DropChoice({
  onImage,
  onFile,
  onCancel,
}: {
  onImage: () => void;
  onFile: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation("files");
  const titleId = useId();
  const first = useRef<HTMLButtonElement>(null);
  const cancel = useRef(onCancel);
  useEffect(() => {
    cancel.current = onCancel;
  });
  useEffect(() => {
    first.current?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      cancel.current();
    };
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("keydown", onKey);
    };
  }, []);
  return (
    <div className="file-drop-choice" role="dialog" aria-modal="true" aria-labelledby={titleId}>
      <p id={titleId} className="file-drop-choice__title">
        {t("composer.dropChoiceTitle")}
      </p>
      <div className="file-drop-choice__actions">
        <button ref={first} type="button" className="file-drop-choice__primary" onClick={onImage}>
          {t("composer.sendAsImage")}
        </button>
        <button type="button" onClick={onFile}>
          {t("composer.shareAsFile")}
        </button>
        <button type="button" onClick={onCancel}>
          {t("composer.dropChoiceCancel")}
        </button>
      </div>
    </div>
  );
}
