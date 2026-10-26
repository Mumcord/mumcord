import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import type { UpdateInfo } from "../../bindings/commands";
import { openInBrowser } from "../chat/openLink";
import "./app.css";

/** Where the hint hears about a newer release. */
export interface UpdateHintApi {
  onUpdate: (handler: (info: UpdateInfo) => void) => () => void;
}

const engineApi: UpdateHintApi = {
  onUpdate: (handler) =>
    getApi().on("updateAvailable", (payload) => {
      handler(payload.info);
    }),
};

/**
 * Small, non-blocking hint that a newer Mumcord exists, with a link to the release page and
 * its update instructions (FR-2.56). Nothing is downloaded or installed. Dismissing hides it
 * until the next start.
 */
export function UpdateHint({ api = engineApi }: { api?: UpdateHintApi }) {
  const { t } = useTranslation("appwindow");
  const [info, setInfo] = useState<UpdateInfo | null>(null);

  useEffect(() => api.onUpdate(setInfo), [api]);

  if (!info) return null;
  return (
    <div role="status" className="update-hint">
      <span className="update-hint__text">
        {t("updateHint.message", { version: info.version })}
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
