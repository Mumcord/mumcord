/**
 * Apps-menu entry **Turn on camera** / **Turn off camera** (FR-8.9, FR-8.19, mockups 01, 02).
 * Available, the second line explains the preview; blocked, it names the reason (FR-8.4).
 */
import { useId } from "react";
import { useTranslation } from "react-i18next";
import { CameraIcon } from "./CameraIcon";
import { useCameraEntry } from "./useCameraEntry";
import "./cameraEntry.css";

export function CameraMenuItem({
  serverId,
  channelName,
  onChosen,
}: {
  serverId: number;
  channelName: string;
  /** Closes the menu; called before the entry acts. */
  onChosen: () => void;
}) {
  const { t } = useTranslation("camera");
  const { onAir, blockText, toggle } = useCameraEntry(serverId);
  const hintId = useId();
  const hint = onAir ? null : (blockText ?? t("turnOnHint", { channel: channelName }));
  return (
    <button
      type="button"
      role="menuitem"
      tabIndex={-1}
      className="server-menu__item chat-apps__item camera-menu-item"
      aria-disabled={blockText !== null || undefined}
      aria-describedby={hint === null ? undefined : hintId}
      onClick={() => {
        if (blockText !== null) return;
        onChosen();
        toggle();
      }}
    >
      <span className="chat-apps__builtin">
        <CameraIcon size={16} off={blockText !== null} />
      </span>
      <span className="chat-apps__text">
        <span className="camera-menu-item__label">{onAir ? t("turnOff") : t("turnOn")}</span>
        {hint !== null && (
          <span id={hintId} className="camera-menu-item__hint">
            {blockText !== null && <InfoIcon />}
            {hint}
          </span>
        )}
      </span>
    </button>
  );
}

function InfoIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      width="12"
      height="12"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      <circle cx="12" cy="12" r="10" />
      <path d="M12 16v-4M12 8h.01" />
    </svg>
  );
}
