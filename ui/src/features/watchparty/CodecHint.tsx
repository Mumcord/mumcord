/**
 * "This video needs H.264 support. Install gst-libav: ‹command›" (FR-2.41a), shown under a player
 * that failed because the webview lacks a decoder.
 *
 * Linux: only shown when the distribution can install gst-libav and the webview really lacks
 * H.264. FreeBSD (FR-13.34 – FR-13.36): the decoder packages WebKitGTK loads at runtime are named
 * with their `pkg install` line; when every installable decoder is present and the player still
 * fails (AV1-only stream, DRM-protected video), the hint offers the browser instead of a command.
 */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  installHint,
  installLine,
  isCodecFailure,
  missingPackages,
  probeCodecs,
  supportsH264,
  type PlaybackFailure,
} from "./codecSupport";
import { useHintPlatform } from "./useHintPlatform";

interface CodecHintProps {
  /** How the player failed, or `null` while it has not. */
  failure: PlaybackFailure | null;
  className?: string;
}

export function CodecHint({ failure, className }: CodecHintProps) {
  const { t } = useTranslation("watchparty");
  const platform = useHintPlatform();
  const [codecs] = useState(() => probeCodecs());
  const [h264] = useState(() => supportsH264());
  if (failure === null || !isCodecFailure(failure) || platform === null) return null;

  if (platform.kind === "freebsd") {
    const missing = missingPackages(codecs);
    const line = installLine(missing);
    return (
      <div className={className} role="status">
        {line === null ? (
          <p>{t("codecHint.freebsdNoPackage")}</p>
        ) : (
          <>
            <p>{t("codecHint.freebsdInstall")}</p>
            <p>
              <code>{line}</code>
            </p>
          </>
        )}
      </div>
    );
  }

  if (h264) return null;
  const hint = installHint(platform.distro);
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
