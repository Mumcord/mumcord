import { useTranslation } from "react-i18next";
import type { InstallKind, UpdateOffer, UpdateStatus } from "../../api";
import { Badge, DialogHeader } from "./DialogHeader";
import { ReleaseNotesList } from "./ReleaseNotesList";
import { packageCommand } from "./updateMode";

export interface AvailableViewProps {
  status: UpdateStatus;
  offer: UpdateOffer;
  titleId: string;
  busy: boolean;
  onDownload: () => void;
  onSkip: () => void;
  onLater: () => void;
  onOpenReleasePage: () => void;
}

/**
 * "Update available": what changed since the installed version and the choice to install it.
 * A copy that cannot install by itself shows how to update it by hand instead.
 */
export function AvailableView(props: AvailableViewProps) {
  const { t } = useTranslation("appwindow");
  const { status, offer } = props;
  const manual = !status.canInstall;
  return (
    <>
      <DialogHeader
        titleId={props.titleId}
        title={t("updater.available.title")}
        subtitle={t(manual ? "updater.available.subtitleManual" : "updater.available.subtitle", {
          version: offer.version,
          current: status.currentVersion,
        })}
        badge={<Badge>{t("updater.stable")}</Badge>}
      />
      <h3 className="upd-section-title">
        {t("updater.available.whatsNew", { current: status.currentVersion })}
      </h3>
      <ReleaseNotesList notes={offer.notes} />
      {manual ? (
        <ManualInstructions install={status.install} version={offer.version} />
      ) : (
        <p className="upd-note-text">{t("updater.available.trust")}</p>
      )}
      <div className="upd-actions">
        <button
          type="button"
          className="upd-text-button"
          disabled={props.busy}
          onClick={props.onSkip}
        >
          {t("updater.available.skip")}
        </button>
        <button type="button" className="upd-text-button" onClick={props.onLater}>
          {t("updater.available.later")}
        </button>
        <button
          type="button"
          className="button button--primary upd-actions__main"
          disabled={props.busy}
          onClick={manual ? props.onOpenReleasePage : props.onDownload}
        >
          {t(manual ? "updater.available.openReleasePage" : "updater.available.download")}
        </button>
      </div>
    </>
  );
}

function ManualInstructions({ install, version }: { install: InstallKind; version: string }) {
  const { t } = useTranslation("appwindow");
  const command = packageCommand(install, version);
  if (command === null) return <p className="upd-note-text">{t("updater.available.manual")}</p>;
  return (
    <div className="upd-manual">
      <p className="upd-note-text">
        {t(install === "aur" ? "updater.available.manualAur" : "updater.available.manualPackage")}
      </p>
      <code className="upd-command">{command}</code>
    </div>
  );
}
