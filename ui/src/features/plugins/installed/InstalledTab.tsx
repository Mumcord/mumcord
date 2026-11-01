import { useCallback, useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { EngineError, getApi, type InstalledPlugin } from "../../../api";
import { Select } from "../../../components/Select";
import {
  consumePluginRequest,
  openPluginTab,
  usePendingPluginRequest,
} from "../pluginRequestStore";
import { usePlugins } from "../pluginStore";
import "./installed.css";
import { usePluginIconUrl } from "../render/assets";
import { BADGE_PERMISSION, displayName, initials, sourceLabel, stateLabel } from "./labels";
import type { RowLabel } from "./labels";
import { PluginDetails } from "./PluginDetails";

/** "" in the badge picker means: no badge next to names. */
const NO_BADGE = "";
/** Error slot of the badge picker (plugin ids never start with a space). */
const BADGE_ERROR = " badge";

/**
 * Settings → Plugins → Installed (FR-6.62, FR-6.63, spec §5.4): one row per plugin with its
 * labels and on/off switch, details behind "⋯", and the "Badge shown next to names" picker.
 */
export function InstalledTab() {
  const { t } = useTranslation("pluginInstalled");
  const { overview } = usePlugins();
  const plugins = overview?.plugins ?? [];
  const badgeId = useId();
  const [openId, setOpenId] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<{ id: string; text: string } | null>(null);
  const rows = useRef(new Map<string, HTMLLIElement>());

  // "Open in Settings" from a notification: open that plugin's details and bring it into view.
  const request = usePendingPluginRequest("installed");
  const [handled, setHandled] = useState(request);
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
  rowRef: (element: HTMLLIElement | null) => void;
  open: boolean;
  onToggleOpen: () => void;
  busy: boolean;
  noPlugins: boolean;
  error: string | null;
  run: (action: () => Promise<unknown>, failedKey: string) => Promise<boolean>;
}

function Label({ label }: { label: RowLabel }) {
  const { t } = useTranslation("pluginInstalled");
  return (
    <span className={`plugins-installed-label plugins-installed-label--${label.tone}`}>
      {t(label.key, label.values ?? {})}
    </span>
  );
}

function PluginRow({
  plugin,
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
  const name = displayName(plugin);
  const iconUrl = usePluginIconUrl(plugin.id, plugin.icon);
  const state = stateLabel(plugin);
  // A revoked plugin cannot be turned on again (the service refuses it).
  const locked = noPlugins || plugin.disabledReason === "revoked";
  return (
    <li ref={rowRef} className="plugins-installed-row">
      <div className="plugins-installed-head">
        <span className="plugins-installed-icon" aria-hidden="true">
          {iconUrl === null ? (
            initials(name)
          ) : (
            <img className="plugins-installed-icon__image" src={iconUrl} alt="" />
          )}
        </span>
        <div className="plugins-installed-title">
          <span className="plugins-installed-name">
            {name} <span className="plugins-installed-version">{plugin.version}</span>
          </span>
          <span className="plugins-installed-labels">
            <Label label={sourceLabel(plugin)} />
            {state !== null && <Label label={state} />}
          </span>
        </div>
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
        <button
          type="button"
          className="settings-button plugins-installed-more"
          aria-label={t("details.open", { name })}
          aria-expanded={open}
          aria-controls={detailsId}
          onClick={onToggleOpen}
        >
          ⋯
        </button>
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
    </li>
  );
}
