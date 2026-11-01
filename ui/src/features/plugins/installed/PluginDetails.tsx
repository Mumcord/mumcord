import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  getApi,
  type InstalledPlugin,
  type PluginSlot,
  type PluginVisibilityChoice,
} from "../../../api";
import { Select } from "../../../components/Select";
import { ConfirmDialog } from "../../settings/components/ConfirmDialog";
import { pickSavePath } from "../../settings/fileDialogs";
import { PermissionList } from "../permissions/PermissionList";
import { PluginTree } from "../render/PluginTree";
import { usePluginTree } from "../render/pluginUiStore";
import { Diagnostics } from "./Diagnostics";
import { MESSAGES_PERMISSION, displayName, requests } from "./labels";
import { RemoveDialog } from "./RemoveDialog";

const SETTINGS_SLOT: PluginSlot = { kind: "settings" };
const VISIBILITIES: readonly PluginVisibilityChoice[] = ["everyone", "friends", "nobody"];

interface PluginDetailsProps {
  plugin: InstalledPlugin;
  /** Runs one action; shows `failedKey` (in `pluginInstalled`) when it throws. */
  run: (action: () => Promise<unknown>, failedKey: string) => Promise<boolean>;
  busy: boolean;
}

/**
 * Everything about one installed plugin (FR-6.62): permissions, visibility, notifications, the
 * plugin's own settings form, Pin version, Diagnostics, Export data, Reset data and Remove.
 */
export function PluginDetails({ plugin, run, busy }: PluginDetailsProps) {
  const { t } = useTranslation("pluginInstalled");
  const baseId = useId();
  const [dialog, setDialog] = useState<"remove" | "reset" | null>(null);
  const [showDiagnostics, setShowDiagnostics] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const settingsTree = usePluginTree(plugin.id, SETTINGS_SLOT);
  const name = displayName(plugin);
  const [taken, setTaken] = useState<{ name: string; takenBy: string }[]>([]);
  useEffect(() => {
    let active = true;
    getApi()
      .pluginCommands()
      .then(
        (commands) => {
          if (active) {
            setTaken(
              commands.flatMap((c) =>
                c.pluginId === plugin.id && c.takenBy !== null
                  ? [{ name: c.name, takenBy: c.takenBy }]
                  : [],
              ),
            );
          }
        },
        (error: unknown) => {
          console.error("Loading plugin commands failed", error);
        },
      );
    return () => {
      active = false;
    };
  }, [plugin.id, plugin.enabled, plugin.granted]);
  const dev = plugin.source.kind === "dev";
  const granted = new Set(plugin.granted);
  const requested = [
    ...plugin.required.map((id) => ({ id, required: true })),
    ...plugin.optional.map((p) => ({ id: p.id, required: false, suggested: p.suggested })),
  ];

  const togglePermission = (permission: string, on: boolean) => {
    const next = on
      ? [...plugin.granted, permission]
      : plugin.granted.filter((id) => id !== permission);
    void run(() => getApi().pluginSetPermissions(plugin.id, next), "errors.permissions");
  };

  const exportData = () =>
    void run(async () => {
      setNotice(null);
      const path = await pickSavePath(
        `${plugin.id}-data.json`,
        { name: t("export.filter"), extensions: ["json"] },
        t("export.title", { name }),
      );
      if (path === null) return;
      await getApi().saveText(path, await getApi().pluginExportData(plugin.id));
      setNotice(t("export.done"));
    }, "errors.export");

  return (
    <div className="plugins-installed-details">
      {requested.length > 0 && (
        <section aria-labelledby={`${baseId}-permissions`}>
          <h4 id={`${baseId}-permissions`}>{t("details.permissions")}</h4>
          <PermissionList
            requested={requested}
            enabled={granted}
            onToggle={togglePermission}
            readOnly={busy}
          />
        </section>
      )}

      {requests(plugin, MESSAGES_PERMISSION) && (
        <div className="plugins-installed-field">
          <label htmlFor={`${baseId}-visibility`}>{t("details.visibility")}</label>
          <Select
            id={`${baseId}-visibility`}
            value={plugin.visibility}
            disabled={busy}
            options={VISIBILITIES.map((value) => ({
              value,
              label: t(`details.visibilityOptions.${value}`),
            }))}
            onChange={(value) =>
              void run(
                () => getApi().pluginSetVisibility(plugin.id, value as PluginVisibilityChoice),
                "errors.visibility",
              )
            }
          />
        </div>
      )}

      <label className="plugins-installed-check" htmlFor={`${baseId}-notifications`}>
        <input
          id={`${baseId}-notifications`}
          type="checkbox"
          role="switch"
          checked={plugin.notifications}
          disabled={busy}
          onChange={(event) => {
            const on = event.target.checked;
            void run(() => getApi().pluginSetNotifications(plugin.id, on), "errors.notifications");
          }}
        />
        {t("details.notifications")}
      </label>

      {!dev && (
        <label className="plugins-installed-check" htmlFor={`${baseId}-pin`}>
          <input
            id={`${baseId}-pin`}
            type="checkbox"
            checked={plugin.pinned}
            disabled={busy}
            onChange={(event) => {
              const pinned = event.target.checked;
              void run(() => getApi().pluginSetPinned(plugin.id, pinned), "errors.pin");
            }}
          />
          {t("details.pin", { version: plugin.version })}
        </label>
      )}

      {taken.length > 0 && (
        <ul className="plugins-installed-taken">
          {taken.map((c) => (
            <li key={c.name}>
              {t("details.commandTaken", { command: c.name, plugin: c.takenBy })}
            </li>
          ))}
        </ul>
      )}

      {settingsTree !== undefined && settingsTree.length > 0 && (
        <section aria-labelledby={`${baseId}-settings`}>
          <h4 id={`${baseId}-settings`}>{t("details.settings")}</h4>
          <PluginTree plugin={plugin.id} slot={SETTINGS_SLOT} nodes={settingsTree} />
        </section>
      )}

      <div className="plugins-installed-actions">
        <button
          type="button"
          className="settings-button"
          aria-expanded={showDiagnostics}
          onClick={() => {
            setShowDiagnostics((shown) => !shown);
          }}
        >
          {t("details.diagnostics")}
        </button>
        <button type="button" className="settings-button" disabled={busy} onClick={exportData}>
          {t("details.export")}
        </button>
        <button
          type="button"
          className="settings-button"
          disabled={busy}
          onClick={() => {
            setDialog("reset");
          }}
        >
          {t("details.reset")}
        </button>
        {!dev && (
          <button
            type="button"
            className="settings-button settings-button--danger"
            disabled={busy}
            onClick={() => {
              setDialog("remove");
            }}
          >
            {t("details.remove")}
          </button>
        )}
      </div>
      {notice !== null && (
        <p role="status" className="settings-hint">
          {notice}
        </p>
      )}
      {showDiagnostics && <Diagnostics id={plugin.id} />}

      {dialog === "reset" && (
        <ConfirmDialog
          title={t("reset.title", { name })}
          message={t("reset.message")}
          confirmLabel={t("reset.confirm")}
          onCancel={() => {
            setDialog(null);
          }}
          onConfirm={() => {
            setDialog(null);
            void run(async () => {
              await getApi().pluginResetData(plugin.id);
              setNotice(t("reset.done"));
            }, "errors.reset");
          }}
        />
      )}
      {dialog === "remove" && (
        <RemoveDialog
          name={name}
          onCancel={() => {
            setDialog(null);
          }}
          onRemove={(deleteData) => {
            setDialog(null);
            void run(() => getApi().pluginRemove(plugin.id, deleteData), "errors.remove");
          }}
        />
      )}
    </div>
  );
}
