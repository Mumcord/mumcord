import { useTranslation } from "react-i18next";
import type { AudioKind } from "./AudioPopover";
import type { useAudioPopover } from "./useAudioPopover";

interface AudioArrowProps {
  kind: AudioKind;
  audio: ReturnType<typeof useAudioPopover>;
  disabled: boolean;
}

/** The small arrow beside the mute or deafen button that opens its audio popover. */
export function AudioArrow({ kind, audio, disabled }: AudioArrowProps) {
  const { t } = useTranslation("userpanel");
  const label = kind === "input" ? t("audio.inputOptions") : t("audio.outputOptions");
  return (
    <button
      type="button"
      className="userpanel__arrow"
      aria-label={label}
      data-tooltip={label}
      aria-haspopup="dialog"
      aria-expanded={audio.isOpen(kind)}
      disabled={disabled}
      onClick={(event) => {
        audio.openFrom(kind, event.currentTarget);
      }}
    >
      <ArrowIcon />
    </button>
  );
}

/** The chevron of the split controls' small popover arrows. */
export function ArrowIcon() {
  return (
    <svg viewBox="0 0 24 24" width="10" height="10" aria-hidden="true" focusable="false">
      <path
        d="M6 9l6 6l6-6"
        fill="none"
        stroke="currentColor"
        strokeWidth="3"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
