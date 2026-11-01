import { useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type InstalledPlugin } from "../../../api";
import { Switch } from "../../settings/components/Switch";
import { isPermissionId, permissionTextKey } from "../permissions/catalogue";

/**
 * Optional permissions an update added (F39). The update never turned them on; the user does
 * here. Suggested ones start switched on, the rest off; nothing changes until a button is pressed.
 * "Not now" keeps them off and clears the offer (they stay available in Installed → details).
 */
export function PermissionOffer({
  plugin,
  onDone,
}: {
  plugin: InstalledPlugin;
  onDone: () => Promise<void>;
}) {
  const { t } = useTranslation("pluginUpdates");
  const { t: tPerm } = useTranslation("pluginPermissions");
  const name = plugin.name.length > 0 ? plugin.name : plugin.id;
  const suggested = (id: string) => plugin.optional.some((p) => p.id === id && p.suggested);
  const [chosen, setChosen] = useState<ReadonlySet<string>>(
    () => new Set(plugin.offered.filter(suggested)),
  );
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const headingId = `plugin-offer-${plugin.id}`;

  const answer = async (turnOn: readonly string[]) => {
    setBusy(true);
    setFailed(false);
    try {
      await getApi().pluginSetPermissions(plugin.id, [...plugin.granted, ...turnOn]);
    } catch (error: unknown) {
      console.error("Answering new plugin permissions failed", error);
      setFailed(true);
    } finally {
      setBusy(false);
      await onDone();
    }
  };

  const toggle = (id: string, on: boolean) => {
    setChosen((current) => {
      const next = new Set(current);
      if (on) next.add(id);
      else next.delete(id);
      return next;
    });
  };

  return (
    <li className="plugin-offer" aria-labelledby={headingId}>
      <p id={headingId} className="plugin-offer__title">
        {t("offer.title", { name, version: plugin.version })}
      </p>
      <ul className="plugin-offer__permissions">
        {plugin.offered.map((id) => {
          const known = isPermissionId(id);
          const text = known ? tPerm(`texts.${permissionTextKey(id)}`) : id;
          return (
            <li key={id}>
              <Switch
                label={t("offer.permission", { permission: text })}
                {...(known && { description: tPerm(`explanations.${permissionTextKey(id)}`) })}
                recommended={suggested(id)}
                checked={chosen.has(id)}
                disabled={busy}
                onChange={(on) => {
                  toggle(id, on);
                }}
              />
            </li>
          );
        })}
      </ul>
      {failed && (
        <p role="alert" className="settings-error">
          {t("offer.failed", { name })}
        </p>
      )}
      <div className="plugin-offer__actions">
        <button
          type="button"
          className="settings-button"
          disabled={busy}
          onClick={() => void answer([])}
        >
          {t("offer.notNow")}
          <span className="plugin-update__sr-only"> {name}</span>
        </button>
        <button
          type="button"
          className="settings-button settings-button--primary"
          disabled={busy}
          onClick={() => void answer(plugin.offered.filter((id) => chosen.has(id)))}
        >
          {t("offer.save")}
          <span className="plugin-update__sr-only"> {name}</span>
        </button>
      </div>
    </li>
  );
}
