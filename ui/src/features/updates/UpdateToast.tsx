import { useId } from "react";
import { useTranslation } from "react-i18next";
import "./updates.css";

export interface UpdateToastProps {
  version: string;
  /** Servers the user is connected to right now. */
  connectedServers: number;
  onRestart: () => void;
  /** Shows the "ready" dialog, which explains what the restart does to the connections. */
  onShowDialog: () => void;
  onLater: () => void;
}

/**
 * Small note that a downloaded update waits for a restart. Restarting while connected first
 * shows the dialog: Mumcord never ends a call without asking.
 */
export function UpdateToast(props: UpdateToastProps) {
  const { t } = useTranslation("appwindow");
  const titleId = useId();
  return (
    <section className="upd-toast" aria-labelledby={titleId}>
      <img className="upd-toast__logo" src={`${import.meta.env.BASE_URL}favicon.png`} alt="" />
      <div className="upd-toast__body">
        <h2 id={titleId} className="upd-toast__title">
          {t("updater.toast.title", { version: props.version })}
        </h2>
        <p className="upd-subtitle">{t("updater.toast.body")}</p>
        <div className="upd-toast__actions">
          <button
            type="button"
            className="button button--primary"
            onClick={props.connectedServers > 0 ? props.onShowDialog : props.onRestart}
          >
            {t("updater.toast.restart")}
          </button>
          <button type="button" className="upd-text-button" onClick={props.onLater}>
            {t("updater.toast.later")}
          </button>
        </div>
      </div>
    </section>
  );
}
