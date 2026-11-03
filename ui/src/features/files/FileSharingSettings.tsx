import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import type { FilesMaxSize } from "../../bindings/commands";
import { Select } from "../../components/Select";
import { Group } from "../settings/components/Section";
import { SettingsIcon } from "../settings/components/SettingsIcon";
import { Switch } from "../settings/components/Switch";
import type { SettingsSectionProps } from "../settings/sections/types";

const SIZES: readonly { value: FilesMaxSize; key: string }[] = [
  { value: "100M", key: "size100mb" },
  { value: "500M", key: "size500mb" },
  { value: "1G", key: "size1gb" },
  { value: "2G", key: "size2gb" },
];

/** Settings → Privacy & Connections → File sharing (spec 07, §7.1 files_*). */
export function FileSharingSettings({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("files");
  const [defaultFolder, setDefaultFolder] = useState<string | null>(null);
  // None means "Mumcord inside the system Downloads folder"; the engine resolves it for display.
  useEffect(() => {
    if (settings.filesSaveFolder !== null) return;
    let live = true;
    void getApi()
      .filesSaveFolder()
      .then((folder) => {
        if (live) setDefaultFolder(folder);
      })
      .catch((error: unknown) => {
        console.error("Could not resolve the default save folder", error);
      });
    return () => {
      live = false;
    };
  }, [settings.filesSaveFolder]);

  const folder = settings.filesSaveFolder ?? defaultFolder ?? "";

  const change = async () => {
    try {
      const picked = await open({
        title: t("settings.saveTo"),
        directory: true,
        multiple: false,
        ...(folder === "" ? {} : { defaultPath: folder }),
      });
      if (picked !== null) update({ filesSaveFolder: picked });
    } catch (error) {
      console.error("Could not pick the save folder", error);
    }
  };

  return (
    <Group
      title={t("settings.title")}
      icon="file"
      badge={<span className="beta-badge">{t("settings.new")}</span>}
    >
      <Switch
        label={t("settings.receive")}
        status={t("settings.receiveHint")}
        checked={settings.filesReceive}
        onChange={(filesReceive) => {
          update({ filesReceive });
        }}
      />
      <div className="settings-control-row">
        <label htmlFor="files-max-size">{t("settings.maxSize")}</label>
        <Select
          id="files-max-size"
          value={settings.filesMaxSize}
          options={SIZES.map(({ value, key }) => ({ value, label: t(`settings.${key}`) }))}
          onChange={(next) => {
            update({ filesMaxSize: next as FilesMaxSize });
          }}
        />
      </div>
      <p className="settings-hint">{t("settings.maxSizeHint")}</p>
      <div className="settings-control-row files-save-row">
        <span id="files-save-label">{t("settings.saveTo")}</span>
        {/* The folder is text, not a field: only the system picker changes it. */}
        <span className="files-save-row__path" id="files-save-folder" data-tooltip={folder}>
          {folder}
        </span>
        <button
          type="button"
          className="settings-button"
          aria-labelledby="files-save-label files-save-change"
          aria-describedby="files-save-folder"
          onClick={() => {
            void change();
          }}
        >
          <span id="files-save-change">{t("settings.change")}</span>
        </button>
      </div>
      <p className="settings-hint files-relay-note">
        <span aria-hidden="true">
          <SettingsIcon name="shield" />
        </span>
        <span>{t("settings.relayNote")}</span>
      </p>
      <p className="settings-hint">{t("settings.encryptionNote")}</p>
    </Group>
  );
}
