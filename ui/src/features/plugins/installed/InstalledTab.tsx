import { useCallback, useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  EngineError,
  getApi,
  type InstalledPlugin,
  type PluginSlot,
  type PluginUpdate,
} from "../../../api";
import { Select } from "../../../components/Select";
import {
  consumePluginRequest,
  openPluginTab,
  usePendingPluginRequest,
} from "../pluginRequestStore";
import { usePlugins } from "../pluginStore";
import { usePluginTree } from "../render/pluginUiStore";
import { useUpdates } from "../updates/useUpdates";
import "./installed.css";
import { InstallProgress } from "../prompt/InstallProgress";
import { usePluginIconUrl } from "../render/assets";
import {
  BADGE_PERMISSION,
  displayName,
  initials,
  permissionChips,
  requests,
  MESSAGES_PERMISSION,
  sourceLabel,
  stateLabel,
} from "./labels";
import { PluginDetails } from "./PluginDetails";
import { RemoveDialog } from "./RemoveDialog";

/** "" in the badge picker means: no badge next to names. */
const NO_BADGE = "";
/** Error slot of the badge picker (plugin ids never start with a space). */
const BADGE_ERROR = " badge";
const SETTINGS_SLOT: PluginSlot = { kind: "settings" };

/**
 * Settings → Plugins → Installed (FR-6.62, FR-6.63, spec §5.4): one card per plugin with its
 * labels, permission chips, on/off switch and inline actions, the details behind "Permissions" or
 * "Settings", and the "Badge shown next to names" picker.
 */
export function InstalledTab() {
  const { t } = useTranslation("pluginInstalled");
  const { overview } = usePlugins();
  const { updates } = useUpdates();
  const plugins = overview?.plugins ?? [];
  const badgeId = useId();
  const [openId, setOpenId] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<{ id: string; text: string } | null>(null);
  const rows = useRef(new Map<string, HTMLLIElement>());

  // "Open in Settings" from a notification: open that plugin's details and bring it into view.
  // A request made while Settings was closed is already waiting when the tab mounts.
  const request = usePendingPluginRequest("installed");
  const [handled, setHandled] = useState<typeof request>(null);
  if (request !== null && request !== handled && overview !== null) {
    setHandled(request);
    if (overview.plugins.some((plugin) => plugin.id === request.plugin)) setOpenId(request.plugin);
  }
  useEffect(() => {
    if (request === null || overview === null) return;
    consumePluginRequest("installed");
    rows.current.get(request.plugin)?.scrollIntoView({ block: "nearest" });
  }, [request, overview]);

  const runFor = useCallback(
    (id: string) =>
      async (action: () => Promise<unknown>, failedKey: string): Promise<boolean> => {
        setBusy(id);
        setError(null);
        try {
          await action();
          return true;
        } catch (cause: unknown) {
          console.error("Plugin action failed", cause);
          const detail = cause instanceof EngineError && cause.message !== "" ? cause.message : "";
          setError({ id, text: `${t(failedKey)} ${detail}`.trim() });
          return false;
        } finally {
          setBusy(null);
        }
      },
    [t],
  );

  if (overview === null) return null;

  if (plugins.length === 0) {
    return (
      <div className="plugins-installed-empty">
        <p>{t("empty")}</p>
        <button
          type="button"
          className="settings-button"
          onClick={() => {
            openPluginTab("browse");
          }}
        >
          {t("browse")}
        </button>
      </div>
    );
  }

  const badgeCandidates = plugins.filter((plugin) => plugin.granted.includes(BADGE_PERMISSION));
  const badge = plugins.find((plugin) => plugin.badge)?.id ?? NO_BADGE;

  return (
    <div className="plugins-installed">
      <ul className="plugins-installed-list" aria-label={t("listLabel")}>
        {plugins.map((plugin) => (
          <PluginRow
            key={plugin.id}
            plugin={plugin}
            update={updates?.find((update) => update.id === plugin.id)}
            rowRef={(element) => {
              if (element) rows.current.set(plugin.id, element);
              else rows.current.delete(plugin.id);
            }}
            open={openId === plugin.id}
            onToggleOpen={() => {
              setOpenId((current) => (current === plugin.id ? null : plugin.id));
            }}
            busy={busy === plugin.id}
            noPlugins={overview.noPlugins}
            error={error?.id === plugin.id ? error.text : null}
            run={runFor(plugin.id)}
          />
        ))}
      </ul>
      {badgeCandidates.length > 0 && (
        <div className="plugins-installed-field">
          <label htmlFor={badgeId}>{t("badge.label")}</label>
          <Select
            id={badgeId}
            value={badge}
            options={[
              ...badgeCandidates.map((plugin) => ({
                value: plugin.id,
                label: displayName(plugin),
              })),
              { value: NO_BADGE, label: t("badge.none") },
            ]}
            onChange={(value) =>
              void runFor(BADGE_ERROR)(
                () => getApi().pluginSetBadge(value === NO_BADGE ? null : value),
                "errors.badge",
              )
            }
          />
          {error?.id === BADGE_ERROR && (
            <p role="alert" className="settings-error">
              {error.text}
            </p>
          )}
        </div>
      )}
    </div>
  );
}

interface PluginRowProps {
  plugin: InstalledPlugin;
  /** The update on offer for this plugin, if any. */
  update: PluginUpdate | undefined;
  rowRef: (element: HTMLLIElement | null) => void;
  open: boolean;
  onToggleOpen: () => void;
  busy: boolean;
  noPlugins: boolean;
  error: string | null;
  run: (action: () => Promise<unknown>, failedKey: string) => Promise<boolean>;
}

function PluginRow({
  plugin,
  update,
  rowRef,
  open,
  onToggleOpen,
  busy,
  noPlugins,
  error,
  run,
}: PluginRowProps) {
  const { t } = useTranslation("pluginInstalled");
  const detailsId = useId();
  const nameId = useId();
  const [removing, setRemoving] = useState(false);
  const name = displayName(plugin);
  const iconUrl = usePluginIconUrl(plugin.id, plugin.icon);
  const state = stateLabel(plugin);
  const hasSettings = (usePluginTree(plugin.id, SETTINGS_SLOT)?.length ?? 0) > 0;
  const chips = permissionChips(plugin);
  const dev = plugin.source.kind === "dev";
  const unofficial = plugin.source.kind === "repo";
  const official = plugin.source.kind === "official";
  const authors = plugin.authors.join(", ");
  // A revoked plugin cannot be turned on again (the service refuses it).
  const locked = noPlugins || plugin.disabledReason === "revoked";
  const source = sourceLabel(plugin);
  const classes = ["plugins-installed-row"];
  if (unofficial) classes.push("plugins-installed-row--unofficial");
  if (state !== null && state.tone === "warning") classes.push("plugins-installed-row--attention");
  // An update that needs the user's approval is reviewed on the Updates tab.
  const applyUpdate = () => {
    if (update === undefined) return;
    if (update.approval !== null) {
      openPluginTab("updates");
      return;
    }
    void run(() => getApi().pluginApplyUpdate(plugin.id, plugin.granted), "errors.update");
  };
  return (
    <li ref={rowRef} className={classes.join(" ")}>
      <div className="plugins-installed-head">
        <span className="plugins-installed-icon" aria-hidden="true">
          {iconUrl === null ? (
            initials(name)
          ) : (
            <img className="plugins-installed-icon__image" src={iconUrl} alt="" />
          )}
        </span>
        <div className="plugins-installed-title">
          <span className="plugins-installed-heading">
            <span id={nameId} className="plugins-installed-name">
              {name}
            </span>
            <span
              className={`plugins-installed-pill plugins-installed-pill--${official ? "ok" : dev ? "neutral" : "warning"}`}
            >
              <span aria-hidden="true">{official ? "✓" : dev ? "" : "⚠"}</span>{" "}
              {t(source.key, source.values ?? {})}
            </span>
            <span className="plugins-installed-version">
              {plugin.version}
              {authors !== "" && ` · ${authors}`}
            </span>
            {update !== undefined && (
              <span className="plugins-installed-pill plugins-installed-pill--warning">
                {t("update.hint", { version: update.to })}
              </span>
            )}
            {plugin.offered.length > 0 && (
              <button
                type="button"
                className="plugins-installed-pill plugins-installed-pill--warning plugins-installed-offer"
                aria-describedby={nameId}
                onClick={() => {
                  openPluginTab("updates");
                }}
              >
                {t("offerBadge")}
              </button>
            )}
          </span>
          {plugin.description !== "" && (
            <span className="plugins-installed-description">{plugin.description}</span>
          )}
          <InstallProgress id={plugin.id} name={name} variant="slim" />
          {(chips.length > 0 || requests(plugin, MESSAGES_PERMISSION)) && (
            <ul className="plugins-installed-chips" aria-label={t("chips.label")}>
              {chips.map((key) => (
                <li key={key} className="plugins-installed-chip">
                  {t(`chips.${key}`)}
                </li>
              ))}
              {requests(plugin, MESSAGES_PERMISSION) && (
                <li className="plugins-installed-chip">
                  {t("visibilityChip", {
                    visibility: t(`details.visibilityOptions.${plugin.visibility}`),
                  })}
                </li>
              )}
            </ul>
          )}
          {state !== null && (
            <span className={`plugins-installed-state plugins-installed-state--${state.tone}`}>
              {t(state.key, state.values ?? {})}
            </span>
          )}
        </div>
        <div className="plugins-installed-side">
          <input
            type="checkbox"
            role="switch"
            className="settings-switch__input"
            aria-label={t("enable", { name })}
            checked={plugin.enabled}
            disabled={busy || locked}
            onChange={(event) => {
              const enabled = event.target.checked;
              void run(() => getApi().pluginSetEnabled(plugin.id, enabled), "errors.enable");
            }}
          />
          <div className="plugins-installed-buttons">
            {update !== undefined && (
              <button
                type="button"
                className="settings-button settings-button--primary"
                aria-describedby={nameId}
                disabled={busy}
                onClick={applyUpdate}
              >
                {t("update.button")}
              </button>
            )}
            <button
              type="button"
              className="settings-button"
              aria-describedby={nameId}
              aria-expanded={open}
              aria-controls={detailsId}
              onClick={onToggleOpen}
            >
              {t("details.permissions")}
            </button>
            {hasSettings && (
              <button
                type="button"
                className="settings-button"
                aria-describedby={nameId}
                aria-expanded={open}
                aria-controls={detailsId}
                onClick={onToggleOpen}
              >
                {t("details.settings")}
              </button>
            )}
            {!dev && (
              <button
                type="button"
                className="plugins-installed-remove"
                aria-describedby={nameId}
                disabled={busy}
                onClick={() => {
                  setRemoving(true);
                }}
              >
                {t("details.remove")}
              </button>
            )}
          </div>
        </div>
      </div>
      {error !== null && (
        <p role="alert" className="settings-error">
          {error}
        </p>
      )}
      {open && (
        <div id={detailsId}>
          <PluginDetails plugin={plugin} run={run} busy={busy} />
        </div>
      )}
      {removing && (
        <RemoveDialog
          name={name}
          onCancel={() => {
            setRemoving(false);
          }}
          onRemove={(deleteData) => {
            setRemoving(false);
            void run(() => getApi().pluginRemove(plugin.id, deleteData), "errors.remove");
          }}
        />
      )}
    </li>
  );
}
