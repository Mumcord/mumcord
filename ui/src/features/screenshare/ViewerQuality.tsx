/**
 * The viewer's quality menu (FR-7.51, mockup 04): what the sharer sends, and the viewer's own
 * frame-rate cap Auto / up to 30 / up to 15. The resolution is the sharer's choice.
 */
import { useId, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { InfoCircleIcon } from "./icons";
import type { ViewerFps } from "./useScreenWatch";
import { useDismiss } from "./useDismiss";

const CHOICES: readonly { fps: ViewerFps; key: "auto" | "upTo30" | "upTo15" }[] = [
  { fps: null, key: "auto" },
  { fps: 30, key: "upTo30" },
  { fps: 15, key: "upTo15" },
];

export interface ViewerQualityProps {
  /** The sharer's name. */
  name: string;
  /** "1440p · 60 fps · AV1" as the sharer sends it; `null` before the first frame. */
  summary: string | null;
  maxFps: ViewerFps;
  onMaxFps: (fps: ViewerFps) => void;
}

export function ViewerQualityMenu({
  name,
  summary,
  maxFps,
  onMaxFps,
}: ViewerQualityProps): ReactNode {
  const { t } = useTranslation("screenviewer");
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const popover = useRef<HTMLDivElement>(null);
  const popoverId = useId();
  const refs = useMemo(() => [button, popover], []);
  useDismiss(refs, open, () => {
    setOpen(false);
    button.current?.focus();
  });
  return (
    <span className="viewer-quality">
      <button
        ref={button}
        type="button"
        className="share-chip share-chip--button viewer-quality__button"
        aria-expanded={open}
        aria-controls={open ? popoverId : undefined}
        onClick={() => {
          setOpen((value) => !value);
        }}
      >
        <InfoCircleIcon />
        {t("quality.label")}
      </button>
      {open && (
        <div
          ref={popover}
          id={popoverId}
          className="viewer-quality__popover"
          role="group"
          aria-label={t("quality.label")}
        >
          <p className="viewer-quality__title">{t("quality.label")}</p>
          {summary !== null && (
            <p className="viewer-quality__set">
              {t("quality.setBySharer", { name })} <strong>{summary}</strong>
            </p>
          )}
          <p className="viewer-quality__label">{t("quality.frameRate")}</p>
          <span
            className="share-segments viewer-quality__segments"
            role="group"
            aria-label={t("quality.frameRate")}
          >
            {CHOICES.map((choice) => (
              <button
                key={choice.key}
                type="button"
                className="share-segments__button"
                aria-pressed={choice.fps === maxFps}
                onClick={() => {
                  onMaxFps(choice.fps);
                }}
              >
                {t(`quality.${choice.key}`)}
              </button>
            ))}
          </span>
          <p className="viewer-quality__hint">{t("quality.hint")}</p>
        </div>
      )}
    </span>
  );
}
