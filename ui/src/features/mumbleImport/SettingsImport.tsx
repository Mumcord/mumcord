import { useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, toEngineError, type MumbleImportFound, type Settings } from "../../api";
import { Group } from "../settings/components/Section";
import { ModalDialog } from "../settings/components/ModalDialog";
import { refreshIdentities } from "../servers/store";
import { MumbleImport } from "./MumbleImport";

type Search =
  | { state: "idle" }
  | { state: "searching" }
  | { state: "notFound" }
  | { state: "failed" }
  | { state: "found"; found: MumbleImportFound };

interface SettingsImportProps {
  settings: Settings;
  update: (patch: Partial<Settings>) => void;
}

/** Settings → Servers → "Import from Mumble" (FR-3.94): looks for the client on request. */
export function SettingsImport({ settings, update }: SettingsImportProps) {
  const { t } = useTranslation("mumbleImport");
  const [search, setSearch] = useState<Search>({ state: "idle" });
  const [finished, setFinished] = useState(false);

  async function look() {
    setSearch({ state: "searching" });
    try {
      const found = await getApi().mumbleImportScan();
      setFinished(false);
      setSearch(found === null ? { state: "notFound" } : { state: "found", found });
    } catch (cause: unknown) {
      console.error("looking for the official Mumble client failed", toEngineError(cause).code);
      setSearch({ state: "failed" });
    }
  }

  const close = () => {
    setSearch({ state: "idle" });
  };

  return (
    <Group title={t("settings.title")}>
      <p className="settings-hint">{t("settings.hint")}</p>
      <div className="settings-control-row">
        <button
          type="button"
          className="button"
          aria-disabled={search.state === "searching"}
          onClick={() => {
            if (search.state !== "searching") void look();
          }}
        >
          {t("settings.button")}
        </button>
      </div>
      {search.state === "searching" && (
        <p className="settings-hint" role="status">
          {t("settings.searching")}
        </p>
      )}
      {search.state === "notFound" && (
        <p className="settings-hint" role="status">
          {t("settings.notFound")}
        </p>
      )}
      {search.state === "failed" && (
        <p className="settings-error" role="alert">
          {t("failed")}
        </p>
      )}
      {search.state === "found" && (
        <ModalDialog
          title={finished ? t("summary.title") : t("settings.title")}
          className="mumble-import-dialog"
          onClose={close}
        >
          <MumbleImport
            found={search.found}
            settings={settings}
            saveSettings={(patch) => {
              update(patch);
              return Promise.resolve();
            }}
            header={(stage) => (
              <>
                <p className="mumble-import__eyebrow">{t("settings.breadcrumb")}</p>
                <p
                  className="mumble-import__heading settings-hint"
                  tabIndex={-1}
                  data-stage-heading
                >
                  {stage === "choose" ? t("intro") : t("summary.intro")}
                </p>
              </>
            )}
            dismissLabel={t("cancel")}
            onDismiss={close}
            onFinish={close}
            onChanged={(done) => {
              setFinished(done !== null);
              // The server bar and the identity list show what the import added or removed.
              void refreshIdentities();
            }}
          />
        </ModalDialog>
      )}
    </Group>
  );
}
