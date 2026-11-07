/**
 * "This video needs H.264 support. Install gst-libav: ‹command›" (FR-2.41a), shown under a player
 * that failed because the webview lacks H.264. Linux only: elsewhere the system decodes H.264.
 */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { installHint, isCodecFailure, supportsH264, type PlaybackFailure } from "./codecSupport";
import { useLinux } from "./useLinux";

interface CodecHintProps {
  /** How the player failed, or `null` while it has not. */
  failure: PlaybackFailure | null;
  className?: string;
}

export function CodecHint({ failure, className }: CodecHintProps) {
  const { t } = useTranslation("watchparty");
  const linux = useLinux();
  const [h264] = useState(() => supportsH264());
  if (failure === null || !isCodecFailure(failure) || linux === null || h264) return null;
  const hint = installHint(linux.distro);
  return (
    <div className={className} role="status">
      {hint.command === null ? (
        <p>{t("codecHint.generic")}</p>
      ) : (
        <p>
          {t("codecHint.install")} <code>{hint.command}</code>
        </p>
      )}
      {hint.rpmFusion && <p>{t("codecHint.rpmFusion")}</p>}
    </div>
  );
}
