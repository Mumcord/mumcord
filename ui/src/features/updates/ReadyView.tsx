import { useTranslation } from "react-i18next";
import type { UpdateOffer, UpdateStatus } from "../../api";
import { Badge, DialogHeader } from "./DialogHeader";
import { StepList } from "./StepList";

export interface ReadyViewProps {
  status: UpdateStatus;
  offer: UpdateOffer;
  connectedServers: number;
  titleId: string;
  busy: boolean;
  onLater: () => void;
  onRestart: () => void;
}

/** The update is verified and in place; it takes effect with the restart the user agrees to. */
export function ReadyView(props: ReadyViewProps) {
  const { t } = useTranslation("appwindow");
  // The Windows installer runs only when Mumcord quits, so the last step is still ahead.
  const installsOnRestart = props.status.install === "windowsInstaller";
  return (
    <>
      <DialogHeader
        titleId={props.titleId}
        title={t("updater.ready.title", { version: props.offer.version })}
        subtitle={t("updater.ready.subtitle")}
        badge={<Badge>{t("updater.ready.verified")}</Badge>}
      />
      <div className="upd-bar" aria-hidden="true">
        <div className="upd-bar__fill" style={{ width: "100%" }} />
      </div>
      <StepList
        steps={[
          { label: t("updater.ready.downloaded"), state: "done" },
          { label: t("updater.ready.checked"), state: "done" },
          installsOnRestart
            ? { label: t("updater.ready.installsOnRestart"), state: "pending" }
            : { label: t("updater.ready.installed"), state: "done" },
        ]}
      />
      {props.connectedServers > 0 && (
        <p className="upd-note-text">
          {t("updater.ready.connected", { count: props.connectedServers })}
        </p>
      )}
      <div className="upd-actions">
        <button type="button" className="upd-text-button" onClick={props.onLater}>
          {t("updater.ready.later")}
        </button>
        <button
          type="button"
          className="button button--primary upd-actions__main"
          disabled={props.busy}
          onClick={props.onRestart}
        >
          {t("updater.ready.restart")}
        </button>
      </div>
    </>
  );
}
