import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  getApi,
  type InstalledPlugin,
  type PluginWebsites,
  type PluginSlot,
  type PluginVisibilityChoice,
} from "../../../api";
import { Select } from "../../../components/Select";
import { ConfirmDialog } from "../../settings/components/ConfirmDialog";
import { DetailSection, OwnCode, PluginPlaces, PluginScoreCard } from "../browse/PluginDetails";
import { usePluginScore } from "../browse/usePluginScore";
import "../browse/details.css";
import { PermissionList } from "../permissions/PermissionList";
import { PluginTree } from "../render/PluginTree";
import { usePluginTree } from "../render/pluginUiStore";
import { Diagnostics } from "./Diagnostics";
import { MESSAGES_PERMISSION, displayName, requests } from "./labels";

const SETTINGS_SLOT: PluginSlot = { kind: "settings" };
const VISIBILITIES: readonly PluginVisibilityChoice[] = ["everyone", "friends", "nobody"];

const RELAY = "peers.relay";

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
  const { t: tOrigins } = useTranslation("pluginOrigins");
  const baseId = useId();
  const [dialog, setDialog] = useState<"reset" | null>(null);
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
  const websites = usePluginWebsites(plugin.id);
  const scored = usePluginScore(
    plugin.id,
    plugin.version,
    "package",
    plugin.source.kind === "official",
  );
  const permissionsHeading = useRef<HTMLHeadingElement>(null);
  const dev = plugin.source.kind === "dev";
  const official = plugin.source.kind === "official";
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
      const data = await getApi().pluginExportData(plugin.id);
      // The engine opens the save dialog itself and writes only where the user chose.
      const saved = await getApi().saveText(
        {
          title: t("export.title", { name }),
          defaultName: `${plugin.id}-data.json`,
          filterName: t("export.filter"),
          extensions: ["json"],
        },
        data,
      );
      if (!saved) return;
      setNotice(t("export.done"));
    }, "errors.export");

  const shown = [...plugin.required, ...plugin.granted];
  const relayRequested = requested.some((p) => p.id === RELAY);

  return (
    <div className="plugins-installed-details">
      {!official && (
        <PluginScoreCard
          name={name}
          scored={scored}
          official={false}
          developer={dev}
          facts={{ websites: websites?.origins.length ?? 0 }}
        />
      )}
      <WebsitesSection name={name} websites={websites} approvedOn={plugin.installedAt} />
      <PluginPlaces permissions={shown} appMenuItems={plugin.appMenuItems} />
      <OwnCode
        permissions={shown}
        webSize={plugin.webSize}
        relay={
          plugin.granted.includes(RELAY) ? (
            tOrigins("details.relayOn")
          ) : relayRequested ? (
            <>
              {tOrigins("details.relayOff")}{" "}
              <button
                type="button"
                className="settings-link"
                onClick={() => {
                  permissionsHeading.current?.focus();
                }}
              >
                {tOrigins("details.turnOnInPermissions")}
              </button>
            </>
          ) : null
        }
      />
      {requested.length > 0 && (
        <section aria-labelledby={`${baseId}-permissions`}>
          <h4 id={`${baseId}-permissions`} ref={permissionsHeading} tabIndex={-1}>
            {t("details.permissions")}
          </h4>
          <PermissionList
            requested={requested}
            enabled={granted}
            pluginName={plugin.name}
            onToggle={togglePermission}
            readOnly={busy}
            chatPatterns={plugin.chatPatterns}
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
    </div>
  );
}

/** The websites an installed plugin declares and its counters (FR-9.30); `null` while loading. */
function usePluginWebsites(id: string): PluginWebsites | null {
  const [websites, setWebsites] = useState<PluginWebsites | null>(null);
  useEffect(() => {
    let active = true;
    getApi()
      .pluginWebsites(id)
      .then(
        (result) => {
          if (active) setWebsites(result);
        },
        (error: unknown) => {
          console.error("Loading the plugin's websites failed", error);
        },
      );
    return () => {
      active = false;
    };
  }, [id]);
  return websites;
}

function localDay(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${String(date.getFullYear())}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/** "Websites it talks to": origins with purpose and today's local counters. */
function WebsitesSection({
  name,
  websites,
  approvedOn,
}: {
  name: string;
  websites: PluginWebsites | null;
  approvedOn: number | null;
}) {
  const { t, i18n } = useTranslation("pluginOrigins");
  if (websites === null || websites.origins.length === 0) return null;
  const today = websites.days.find((d) => d.day === localDay(new Date()));
  const last = websites.days.find((d) => d.lastRequestAt !== null);
  const date =
    approvedOn !== null && approvedOn > 0
      ? new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium" }).format(new Date(approvedOn))
      : null;
  return (
    <DetailSection title={t("details.title")}>
      <p className="pd-hint">
        {date !== null ? t("details.text", { plugin: name, date }) : t("intro", { plugin: name })}
      </p>
      <ul className="ps-origins" aria-label={t("listLabel")}>
        {websites.origins.map((site) => (
          <li key={site.origin} className="ps-origin">
            <code>{site.origin}</code>
            <span>{site.purpose}</span>
          </li>
        ))}
      </ul>
      <dl className="ps-facts">
        <dt>{t("details.lastRequest")}</dt>
        <dd>
          {last?.lastRequestAt != null
            ? [
                new Intl.DateTimeFormat(i18n.language, {
                  dateStyle: "medium",
                  timeStyle: "short",
                }).format(new Date(last.lastRequestAt)),
                last.lastHost,
              ]
                .filter((part) => part !== null)
                .join(" · ")
            : t("details.none")}
        </dd>
        <dt>{t("details.requestsToday")}</dt>
        <dd>
          {t("details.requestsTodayValue", {
            count: today?.requests ?? 0,
            blocked: today?.blocked ?? 0,
          })}
        </dd>
      </dl>
    </DetailSection>
  );
}
