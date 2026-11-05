/**
 * The sound line in the sharer's live bar (FR-7.85, mockup 10): "Sound: desktop", "Sound: {app}"
 * or "No sound". A tooltip says what the sound carries or why there is none; the same text is
 * the badge's accessible description, so screen reader users get the reason too.
 */
import { useId, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { ScreenShare } from "../../../api";
import { useAppOs } from "../useAppOs";
import { SpeakerIcon } from "../icons";
import { sendsSound, soundTip } from "./soundTip";

export function SoundBadge({ own }: { own: ScreenShare }): ReactNode {
  const { t } = useTranslation("screenshare");
  const os = useAppOs();
  const reasonId = useId();
  const hasSound = sendsSound(own);
  const label = !hasSound
    ? t("liveBar.noSound")
    : own.soundMode === "app" && own.soundApp !== null
      ? t("liveBar.soundApp", { app: own.soundApp })
      : t("liveBar.soundDesktop");
  const tip = soundTip(own, os, t);
  const warn = !hasSound && own.soundReason === "captureFailed";

  return (
    <span className="share-badges">
      {/* Focusable so keyboard users can open the tooltip, as in mockup 10. */}
      <span
        className="share-badge"
        // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex
        tabIndex={0}
        data-tone={warn ? "warn" : undefined}
        data-tooltip={tip}
        data-tooltip-placement="bottom"
        aria-describedby={reasonId}
      >
        <SpeakerIcon muted={!hasSound} size={12} />
        {label}
      </span>
      <span id={reasonId} className="visually-hidden">
        {tip}
      </span>
    </span>
  );
}
