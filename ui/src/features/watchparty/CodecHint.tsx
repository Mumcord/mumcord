/**
 * "This video needs H.264 support. Install gst-libav: ‹command›" (FR-2.41a), shown under a player
 * that failed because the webview lacks H.264. Linux only: elsewhere the system decodes H.264.
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type LinuxDistro } from "../../api";
import { installHint, isCodecFailure, supportsH264, type PlaybackFailure } from "./codecSupport";

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

/** The distribution when the app runs on Linux, `null` elsewhere and until it is known. */
function useLinux(): { distro: LinuxDistro | null } | null {
  const [linux, setLinux] = useState<{ distro: LinuxDistro | null } | null>(null);
  useEffect(() => {
    let active = true;
    getApi()
      .appInfo()
      .then(
        (info) => {
          if (active && info.os.toLowerCase().startsWith("linux")) {
            setLinux({ distro: info.distro });
          }
        },
        (error: unknown) => {
          console.error("Reading the app info failed", error);
        },
      );
    return () => {
      active = false;
    };
  }, []);
  return linux;
}
