import { useTranslation } from "react-i18next";

export interface JumpToChannelButtonProps {
  /** Not joined for voice (reading along or not synced yet): there is no channel to jump to. */
  disabled: boolean;
  onJump: () => void;
}

/** "Jump to my channel" in the row above the channel tree (QA.27). */
export function JumpToChannelButton({ disabled, onJump }: JumpToChannelButtonProps) {
  const { t } = useTranslation("channels");
  const label = t("tree.jump");
  return (
    <button
      type="button"
      className="jump-to-channel"
      aria-label={label}
      title={label}
      // aria-disabled keeps the button focusable and its tooltip readable while there is no call.
      aria-disabled={disabled}
      onClick={() => {
        if (!disabled) onJump();
      }}
    >
      {/* Crosshair icon; the button's name is its translated aria-label. */}
      <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" focusable="false">
        <g fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
          <circle cx="12" cy="12" r="7" />
          <circle cx="12" cy="12" r="2" fill="currentColor" />
          <path d="M12 2v3M12 19v3M2 12h3M19 12h3" />
        </g>
      </svg>
    </button>
  );
}
