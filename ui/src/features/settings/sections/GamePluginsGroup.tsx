import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type GamePlugin, type PositionalStatus } from "../../../api";
import { BetaBadge } from "../../../components/BetaBadge";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";

/** Game plugins and the positional status, kept up to date while the group is shown. */
function useGamePlugins() {
  const [plugins, setPlugins] = useState<GamePlugin[] | null>(null);
  const [status, setStatus] = useState<PositionalStatus | null>(null);
  const [error, setError] = useState<"load" | "switch" | null>(null);

  useEffect(() => {
    let active = true;
    const api = getApi();
    api.pluginsList().then(
      (loaded) => {
        if (active) setPlugins(loaded);
      },
      (e: unknown) => {
        console.error("Listing the game plugins failed", e);
        if (active) setError("load");
      },
    );
    api.positionalStatus().then(
      (loaded) => {
        if (active) setStatus(loaded);
      },
      (e: unknown) => {
        console.error("Reading the positional audio status failed", e);
      },
    );
    const off = api.on("positionalStatusChanged", (event) => {
      setStatus(event.status);
    });
    return () => {
      active = false;
      off();
    };
  }, []);

  const setEnabled = (id: string, enabled: boolean) => {
    setError(null);
    getApi()
      .pluginSetEnabled(id, enabled)
      .then(setPlugins, (e: unknown) => {
        console.error("Switching a game plugin failed", e);
        setError("switch");
      });
  };

  return { plugins, status, error, setEnabled };
}

/**
 * Positional audio sources (FR-1.42b): the official Mumble game plugins, each switched on individually, with the anti-cheat
 * notice. Plugin names come from third-party libraries and are shown as plain text.
 */
export function GamePluginsGroup() {
  const { t } = useTranslation("settings");
  const { plugins, status, error, setEnabled } = useGamePlugins();

  // Spread into the switch: it takes no `status` at all when there is nothing to say.
  const pluginStatus = (p: GamePlugin): { status?: string } => {
    const parts: string[] = [];
    if (p.version) parts.push(t("gamePlugins.version", { version: p.version }));
    if (p.problem) parts.push(t(`gamePlugins.problem.${p.problem}`));
    else if (p.enabled && status?.source === p.name) parts.push(t("gamePlugins.active"));
    return parts.length > 0 ? { status: parts.join(" · ") } : {};
  };

  return (
    <Group
      title={t("gamePlugins.title")}
      icon="box"
      badge={<BetaBadge />}
      info={`${t("gamePlugins.explanation")} ${t("gamePlugins.antiCheat")}`}
      wide
    >
      {plugins === null && error === null && (
        <p className="settings-hint">{t("gamePlugins.loading")}</p>
      )}
      {plugins?.length === 0 && <p className="settings-hint">{t("gamePlugins.none")}</p>}
      {plugins?.map((p) => (
        <Switch
          key={p.id}
          label={p.name}
          {...pluginStatus(p)}
          checked={p.enabled}
          disabled={p.problem === "incompatible"}
          onChange={(on) => {
            setEnabled(p.id, on);
          }}
        />
      ))}
      {error !== null && (
        <p role="alert" className="settings-error">
          {t(error === "load" ? "gamePlugins.loadFailed" : "gamePlugins.switchFailed")}
        </p>
      )}
    </Group>
  );
}
