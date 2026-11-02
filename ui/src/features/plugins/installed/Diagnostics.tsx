import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type PluginDiagnostics } from "../../../api";
import { formatSize } from "../../settings/formatSize";

type Counter = Exclude<
  keyof PluginDiagnostics,
  "log" | "cpuTimeMs" | "memoryBytes" | "peakMemoryBytes"
>;

const COUNTERS: readonly Counter[] = [
  "droppedEvents",
  "droppedMessages",
  "restarts",
  "messagesSent",
  "messagesReceived",
  "messagesRateLimited",
  "messagesExpired",
  "incomingRateDropped",
  "incomingInvalid",
];

/**
 * Diagnostics of one plugin (FR-6.8): CPU time, memory and the counters, read once when opened.
 * The plugin's own log is only shown in Developer mode.
 */
export function Diagnostics({ id }: { id: string }) {
  const { t } = useTranslation("pluginInstalled");
  const [data, setData] = useState<PluginDiagnostics | null>(null);
  const [developerMode, setDeveloperMode] = useState(false);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let active = true;
    Promise.all([getApi().pluginDiagnostics(id), getApi().settingsGet()]).then(
      ([diagnostics, settings]) => {
        if (!active) return;
        setData(diagnostics);
        setDeveloperMode(settings.developerMode);
      },
      (cause: unknown) => {
        console.error("Reading the plugin diagnostics failed", cause);
        if (active) setFailed(true);
      },
    );
    return () => {
      active = false;
    };
  }, [id]);

  if (failed) {
    return (
      <p role="alert" className="settings-error">
        {t("diagnostics.failed")}
      </p>
    );
  }
  if (data === null) return <p className="settings-hint">{t("diagnostics.loading")}</p>;

  const value = (n: number | null) => (n === null ? t("diagnostics.unknown") : String(n));
  const size = (n: number | null) => (n === null ? t("diagnostics.unknown") : formatSize(n));
  const rows: [string, string][] = [
    [
      "cpuTime",
      data.cpuTimeMs === null
        ? t("diagnostics.unknown")
        : t("diagnostics.ms", { value: data.cpuTimeMs }),
    ],
    ["memory", size(data.memoryBytes)],
    ["peakMemory", size(data.peakMemoryBytes)],
    ...COUNTERS.map((key): [string, string] => [key, value(data[key])]),
  ];
  return (
    <div className="plugins-installed-diagnostics">
      <dl>
        {rows.map(([key, shown]) => (
          <div key={key} className="plugins-installed-diagnostics-pair">
            <dt>{t(`diagnostics.${key}`)}</dt>
            <dd>{shown}</dd>
          </div>
        ))}
      </dl>
      {developerMode && (
        <section aria-label={t("diagnostics.log")}>
          <h4>{t("diagnostics.log")}</h4>
          {data.log.length === 0 ? (
            <p className="settings-hint">{t("diagnostics.logEmpty")}</p>
          ) : (
            <ol className="plugins-installed-log">
              {data.log.map((line, index) => (
                <li key={index}>
                  <span className="plugins-installed-log-level">
                    {t(`diagnostics.level.${line.level}`)}
                  </span>{" "}
                  {line.text}
                </li>
              ))}
            </ol>
          )}
        </section>
      )}
    </div>
  );
}
