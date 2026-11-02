import { useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type PluginUpdates } from "../../../api";
import { RadioGroup } from "../../settings/components/RadioGroup";
import { Switch } from "../../settings/components/Switch";
import { openPluginTab } from "../pluginRequestStore";
import { refreshPlugins, usePlugins } from "../pluginStore";
import { PermissionOffer } from "./PermissionOffer";
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
  const { t, i18n } = useTranslation("pluginUpdates");
  const { overview } = usePlugins();
  const { updates, loadFailed, reload } = useUpdates();
  const plugins = overview?.plugins ?? [];
  // Developer plugins are loaded from a folder and have no versions to pin.
  const pinnable = plugins.filter((p) => p.source.kind !== "dev");

  const revoked = plugins.filter((p) => p.disabledReason === "revoked");
  const offers = plugins.filter((p) => p.offered.length > 0);
  const [checking, setChecking] = useState(false);
  const [checkFailed, setCheckFailed] = useState(false);
  const [checkedAt, setCheckedAt] = useState<Date | null>(null);

  const checkNow = async () => {
    setChecking(true);
    setCheckFailed(false);
    try {
      await getApi().pluginCheckNow();
      setCheckedAt(new Date());
    } catch (error: unknown) {
      console.error("Checking for plugin updates failed", error);
      setCheckFailed(true);
    } finally {
      setChecking(false);
      await Promise.all([reload(), refreshPlugins()]);
    }
  };

  const removeRevoked = async (id: string) => {
    try {
      // The data stays, as the banner promises; Installed offers deleting it.
      await getApi().pluginRemove(id, false);
    } catch (error: unknown) {
      console.error("Removing a revoked plugin failed", error);
    }
    await refreshPlugins();
  };

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
      {revoked.map((plugin) => {
        const name = plugin.name.length > 0 ? plugin.name : plugin.id;
        return (
          <div key={plugin.id} role="alert" className="plugin-revocation">
            <span className="plugin-revocation__mark" aria-hidden="true">
              ⛔
            </span>
            <div className="plugin-revocation__text">
              <strong>{t("revoked.title", { name })}</strong>
              <span>
                {t("revoked.text", { version: plugin.version, reason: plugin.revocation ?? "" })}
              </span>
            </div>
            <button
              type="button"
              className="plugin-revocation__link"
              onClick={() => {
                openPluginTab("installed");
              }}
            >
              {t("revoked.details")}
              <span className="plugin-update__sr-only"> {name}</span>
            </button>
            <button
              type="button"
              className="settings-button"
              onClick={() => void removeRevoked(plugin.id)}
            >
              {t("revoked.remove")}
              <span className="plugin-update__sr-only"> {name}</span>
            </button>
          </div>
        );
      })}
      {offers.length > 0 && (
        <section aria-labelledby="plugin-updates-offers">
          <h3 id="plugin-updates-offers" className="plugin-updates__heading">
            {t("offer.heading")}
          </h3>
          <ul className="plugin-updates__list">
            {offers.map((plugin) => (
              <PermissionOffer
                key={`${plugin.id} ${plugin.offered.join(" ")}`}
                plugin={plugin}
                onDone={onDone}
              />
            ))}
          </ul>
        </section>
      )}
      <section aria-labelledby="plugin-updates-waiting">
        <h3 id="plugin-updates-waiting" className="plugin-update__sr-only">
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
      <div className="plugin-updates__footer">
        <p role="status" className="plugin-updates__checked">
          {checkFailed
            ? t("checkFailed")
            : checkedAt !== null
              ? t("lastChecked", { time: checkedAt.toLocaleTimeString(i18n.language) })
              : t("modeNote", { mode: t(`mode.${mode}`) })}
        </p>
        <button
          type="button"
          className="settings-button"
          disabled={checking}
          onClick={() => void checkNow()}
        >
          {t("checkNow")}
        </button>
      </div>
      <details className="plugin-updates__settings">
        <summary>{t("settings")}</summary>
        <RadioGroup
          legend={t("mode.legend")}
          info={{ label: t("mode.infoLabel"), text: t("mode.info") }}
          value={mode}
          options={MODES.map((value) => ({ value, label: t(`mode.${value}`) }))}
          onChange={onModeChange}
        />
        {pinnable.length > 0 && (
          <section aria-labelledby="plugin-updates-pins">
            <h3 id="plugin-updates-pins" className="plugin-updates__heading">
              {t("pins")}
            </h3>
            <p className="settings-hint">{t("pinsHint")}</p>
            <ul className="plugin-updates__list">
              {pinnable.map((plugin) => (
                <li key={plugin.id} className="plugin-pin">
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
      </details>
    </div>
  );
}
