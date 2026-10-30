import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import type { UpdateInfo } from "../../bindings/commands";
import { openInBrowser } from "../chat/openLink";
import "./app.css";

/** Where the hint hears about a newer release. */
export interface UpdateHintApi {
  onUpdate: (handler: (info: UpdateInfo) => void) => () => void;
  /** Whether this is the portable Windows build, which is updated by a new zip. */
  isPortable: () => Promise<boolean>;
}

const engineApi: UpdateHintApi = {
  onUpdate: (handler) =>
    getApi().on("updateAvailable", (payload) => {
      handler(payload.info);
    }),
  isPortable: () =>
    getApi()
      .appInfo()
      .then((info) => info.portable),
};

/**
 * Small, non-blocking hint that a newer Mumcord exists, with a link to the release page and
 * its update instructions (FR-2.56). Nothing is downloaded or installed. Dismissing hides it
 * until the next start. The portable build asks for the new portable zip instead (#181).
 */
export function UpdateHint({ api = engineApi }: { api?: UpdateHintApi }) {
  const { t } = useTranslation("appwindow");
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [portable, setPortable] = useState(false);

  useEffect(() => api.onUpdate(setInfo), [api]);
  useEffect(() => {
    let active = true;
    api.isPortable().then(
      (value) => {
        if (active) setPortable(value);
      },
      (error: unknown) => {
        console.error("Loading app info failed", error);
      },
    );
    return () => {
      active = false;
    };
  }, [api]);

  if (!info) return null;
  return (
    <div role="status" className="update-hint">
      <span className="update-hint__text">
        {t("updateHint.message", { version: info.version })}
        {portable && ` ${t("updateHint.portable")}`}
      </span>
      <button
        type="button"
        className="button button--primary"
        onClick={() => {
          openInBrowser(info.url);
        }}
      >
        {t("updateHint.open")}
      </button>
      <button
        type="button"
        className="button"
        onClick={() => {
          setInfo(null);
        }}
      >
        {t("updateHint.dismiss")}
      </button>
    </div>
  );
}
