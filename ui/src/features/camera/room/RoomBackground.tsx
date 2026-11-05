/**
 * The room bar's "Background: {name}" button (FR-8.40, mockups 05–10). It opens the preview's
 * background picker as a popover and changes the background of the running camera without
 * stopping the broadcast and without saving it as the default.
 */
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { BackgroundPicker } from "../preview/BackgroundPicker";
import { applyLiveBackground, useLiveBackground } from "../preview/liveBackground";
import { backgroundMode } from "../preview/previewLogic";
import { useBackgroundLibrary } from "../preview/useBackgroundLibrary";
import { useEffectsLoad } from "../preview/useEffectsLoad";
import { useDismiss } from "../../screenshare/useDismiss";
import { RoomIcon } from "./icons";
import "../preview/preview.css";

const MODE_KEYS = {
  none: "preview.backgroundNone",
  blur: "preview.backgroundBlur",
  image: "preview.backgroundImage",
} as const;

/** Space between the button and the popover, and the least distance to the window edge. */
const GAP = 8;

export interface RoomBackgroundProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function RoomBackground({ open, onOpenChange }: RoomBackgroundProps) {
  const { t } = useTranslation("camera");
  const live = useLiveBackground();
  const buttonRef = useRef<HTMLButtonElement>(null);
  const popoverRef = useRef<HTMLDivElement>(null);
  const refs = useMemo(() => [buttonRef, popoverRef], []);
  const close = useCallback(() => {
    onOpenChange(false);
    buttonRef.current?.focus();
  }, [onOpenChange]);
  useDismiss(refs, open, close);

  return (
    <>
      <button
        ref={buttonRef}
        type="button"
        className="cam-btn"
        aria-haspopup="dialog"
        aria-expanded={open}
        onClick={() => {
          onOpenChange(!open);
        }}
      >
        <RoomIcon name="background" />
        {t("room.background", { name: t(MODE_KEYS[backgroundMode(live.background)]) })}
        <RoomIcon name="chevron" />
      </button>
      {open && <BackgroundPopover anchor={buttonRef} popoverRef={popoverRef} />}
    </>
  );
}

function BackgroundPopover({
  anchor,
  popoverRef,
}: {
  anchor: RefObject<HTMLButtonElement | null>;
  popoverRef: RefObject<HTMLDivElement | null>;
}) {
  const { t } = useTranslation("camera");
  const live = useLiveBackground();
  const effects = useEffectsLoad();
  const own = useBackgroundLibrary();

  // Above the button, kept inside the window; the bar sits at the bottom of the room.
  useLayoutEffect(() => {
    const element = popoverRef.current;
    const button = anchor.current;
    if (element === null || button === null) return;
    const rect = button.getBoundingClientRect();
    const left = Math.max(GAP, Math.min(rect.left, window.innerWidth - element.offsetWidth - GAP));
    const top = Math.max(GAP, rect.top - GAP - element.offsetHeight);
    element.style.left = `${String(left)}px`;
    element.style.top = `${String(top)}px`;
  });

  useEffect(() => {
    popoverRef.current
      ?.querySelector<HTMLElement>("input:checked, input:not(:disabled), button")
      ?.focus();
  }, [popoverRef]);

  return (
    <div
      ref={popoverRef}
      className="cam-bg-popover"
      role="dialog"
      aria-label={t("room.backgroundPicker")}
    >
      <BackgroundPicker
        background={live.background}
        blurStrength={live.blurStrength}
        library={own.library}
        effectsOff={effects?.unavailable != null}
        adding={own.adding}
        addError={own.addFailed ? t("imageImportFailed") : null}
        onBackground={(background) => {
          applyLiveBackground({ background, blurStrength: live.blurStrength });
        }}
        onBlurStrength={(blurStrength) => {
          applyLiveBackground({ background: live.background, blurStrength });
        }}
        onAddImage={() => {
          void own.add().then((added) => {
            if (added !== null)
              applyLiveBackground({
                background: { kind: "own", id: added.id },
                blurStrength: live.blurStrength,
              });
          });
        }}
      />
    </div>
  );
}
