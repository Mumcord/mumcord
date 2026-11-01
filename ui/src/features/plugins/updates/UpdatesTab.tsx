import { useTranslation } from "react-i18next";
import { getApi, type PluginUpdates } from "../../../api";
import { RadioGroup } from "../../settings/components/RadioGroup";
import { Switch } from "../../settings/components/Switch";
import { refreshPlugins, usePlugins } from "../pluginStore";
import { UpdateRow } from "./UpdateRow";
import { useUpdates } from "./useUpdates";
import "./updates.css";

const MODES: readonly PluginUpdates[] = ["auto", "notify", "off"];

/**
 * Settings → Plugins → Updates (FR-6.66–6.69): the plugin update setting, the updates waiting
 * for the user, and "Pin version" for every installed plugin.
 */
export function UpdatesTab({
  mode,
  onModeChange,
}: {
  mode: PluginUpdates;
  onModeChange: (mode: PluginUpdates) => void;
}) {
  const { t } = useTranslation("pluginUpdates");
  const { overview } = usePlugins();
  const { updates, loadFailed, reload } = useUpdates();
  const plugins = overview?.plugins ?? [];
  // Developer plugins are loaded from a folder and have no versions to pin.
  const pinnable = plugins.filter((p) => p.source.kind !== "dev");

  const onDone = async () => {
    await Promise.all([reload(), refreshPlugins()]);
  };

  const setPinned = async (id: string, pinned: boolean) => {
    try {
      await getApi().pluginSetPinned(id, pinned);
    } catch (error: unknown) {
      console.error("Changing 'pin version' failed", error);
    }
    await onDone();
  };

  return (
    <div className="plugin-updates">
      <RadioGroup
        legend={t("mode.legend")}
        info={{ label: t("mode.infoLabel"), text: t("mode.info") }}
        value={mode}
        options={MODES.map((value) => ({ value, label: t(`mode.${value}`) }))}
        onChange={onModeChange}
      />
      <section aria-labelledby="plugin-updates-waiting">
        <h3 id="plugin-updates-waiting" className="plugin-updates__heading">
          {t("waiting")}
        </h3>
        {loadFailed && (
          <p role="alert" className="settings-error">
            {t("loadFailed")}
          </p>
        )}
        {updates !== null && updates.length === 0 && (
          <p className="plugin-updates__empty">{mode === "off" ? t("emptyOff") : t("empty")}</p>
        )}
        {updates !== null && updates.length > 0 && (
          <ul className="plugin-updates__list">
            {updates.map((update) => (
              <UpdateRow
                key={update.id}
                update={update}
                installed={plugins.find((p) => p.id === update.id)}
                onDone={onDone}
              />
            ))}
          </ul>
        )}
      </section>
      {pinnable.length > 0 && (
        <section aria-labelledby="plugin-updates-pins">
          <h3 id="plugin-updates-pins" className="plugin-updates__heading">
            {t("pins")}
          </h3>
          <p className="settings-hint">{t("pinsHint")}</p>
          <ul className="plugin-updates__list">
            {pinnable.map((plugin) => (
              <li key={plugin.id} className="plugin-update">
                <Switch
                  label={t("pin", { name: plugin.name.length > 0 ? plugin.name : plugin.id })}
                  description={t("pinVersion", { version: plugin.version })}
                  checked={plugin.pinned}
                  onChange={(pinned) => void setPinned(plugin.id, pinned)}
                />
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}
