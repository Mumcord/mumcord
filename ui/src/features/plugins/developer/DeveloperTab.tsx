import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { EngineError, getApi, type InstalledPlugin, type PluginLogLine } from "../../../api";
import { usePlugins } from "../pluginStore";
import "./developer.css";
import { pickFolderPath } from "./pickFolder";

const LOG_POLL_MS = 2000;

/** The error to show for a failed load/reload/unload: a fixed text plus the engine's detail. */
function failure(prefix: string, error: unknown): string {
  return error instanceof EngineError && error.message !== ""
    ? `${prefix} ${error.message}`
    : prefix;
}

/**
 * Settings → Plugins → Developer (FR-6.73): load an unpacked plugin folder, reload or unload it
 * and read its log. The tab only exists while Developer mode is on.
 */
export function DeveloperTab() {
  const { t } = useTranslation("pluginDeveloper");
  const { overview } = usePlugins();
  const devPlugins = (overview?.plugins ?? []).filter((plugin) => plugin.source.kind === "dev");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const selected = devPlugins.find((plugin) => plugin.id === selectedId) ?? devPlugins[0] ?? null;

  const run = useCallback(async (action: () => Promise<unknown>, failed: string) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (cause: unknown) {
      console.error("Developer plugin action failed", cause);
      setError(failure(failed, cause));
    } finally {
      setBusy(false);
    }
  }, []);

  const load = () =>
    void run(async () => {
      const folder = await pickFolderPath(t("pickFolder"));
      if (folder === null) return;
      setSelectedId(await getApi().pluginDevLoad(folder));
    }, t("errors.load"));

  return (
    <div className="plugins-developer">
      <p className="plugins-developer-hint">{t("hint")}</p>
      <div>
        <button type="button" className="settings-button" disabled={busy} onClick={load}>
          {t("load")}
        </button>
      </div>
      {error !== null && (
        <p role="alert" className="settings-error">
          {error}
        </p>
      )}
      {devPlugins.length === 0 ? (
        <p className="plugins-developer-empty">{t("empty")}</p>
      ) : (
        <ul className="plugins-developer-list" aria-label={t("listLabel")}>
          {devPlugins.map((plugin) => (
            <DeveloperRow
              key={plugin.id}
              plugin={plugin}
              selected={plugin.id === selected?.id}
              busy={busy}
              onSelect={() => {
                setSelectedId(plugin.id);
              }}
              onReload={() =>
                void run(() => getApi().pluginDevReload(plugin.id), t("errors.reload"))
              }
              onUnload={() =>
                void run(() => getApi().pluginDevUnload(plugin.id), t("errors.unload"))
              }
            />
          ))}
        </ul>
      )}
      {selected !== null && <PluginLog key={selected.id} plugin={selected} />}
    </div>
  );
}

function DeveloperRow({
  plugin,
  selected,
  busy,
  onSelect,
  onReload,
  onUnload,
}: {
  plugin: InstalledPlugin;
  selected: boolean;
  busy: boolean;
  onSelect: () => void;
  onReload: () => void;
  onUnload: () => void;
}) {
  const { t } = useTranslation("pluginDeveloper");
  const name = plugin.name === "" ? plugin.id : plugin.name;
  return (
    <li className="plugins-developer-row" aria-current={selected}>
      <div className="plugins-developer-info">
        <strong>{name}</strong>
        {/* Permanent label: a developer plugin is never mistaken for an installed one (FR-6.73). */}
        <span className="plugins-developer-badge">{t("label")}</span>
        <span className="plugins-developer-status">{t(`status.${plugin.status.kind}`)}</span>
        {plugin.source.kind === "dev" && (
          <code className="plugins-developer-folder">{plugin.source.folder}</code>
        )}
      </div>
      <div className="plugins-developer-actions">
        <button
          type="button"
          className="settings-button"
          aria-pressed={selected}
          onClick={onSelect}
        >
          {t("showLog", { name })}
        </button>
        <button type="button" className="settings-button" disabled={busy} onClick={onReload}>
          {t("reload", { name })}
        </button>
        <button type="button" className="settings-button" disabled={busy} onClick={onUnload}>
          {t("unload", { name })}
        </button>
      </div>
    </li>
  );
}

/** The plugin's in-memory log, refreshed while the tab is open. */
function PluginLog({ plugin }: { plugin: InstalledPlugin }) {
  const { t } = useTranslation("pluginDeveloper");
  const [lines, setLines] = useState<PluginLogLine[]>([]);
  const [failed, setFailed] = useState(false);
  const name = plugin.name === "" ? plugin.id : plugin.name;

  useEffect(() => {
    let cancelled = false;
    const refresh = () => {
      getApi()
        .pluginDiagnostics(plugin.id)
        .then((diagnostics) => {
          if (cancelled) return;
          setLines(diagnostics.log);
          setFailed(false);
        })
        .catch((cause: unknown) => {
          console.error("Reading the plugin log failed", cause);
          if (!cancelled) setFailed(true);
        });
    };
    refresh();
    const timer = window.setInterval(refresh, LOG_POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [plugin.id]);

  return (
    <section className="plugins-developer-log" aria-label={t("logLabel", { name })}>
      <h3>{t("logTitle", { name })}</h3>
      {failed && (
        <p role="alert" className="settings-error">
          {t("errors.log")}
        </p>
      )}
      {lines.length === 0 ? (
        <p className="plugins-developer-empty">{t("logEmpty")}</p>
      ) : (
        <ol className="plugins-developer-lines">
          {lines.map((line, index) => (
            <li key={index} className={`plugins-developer-line plugins-developer-${line.level}`}>
              <span className="plugins-developer-level">{t(`level.${line.level}`)}</span>{" "}
              {line.text}
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}
