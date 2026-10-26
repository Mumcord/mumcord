import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../../api";
import { Switch } from "../components/Switch";

/**
 * AppImage only: the single switch for the app-menu entry (FR-2.55). Turning it on writes the
 * entry; turning it off stops Mumcord from writing it again.
 */
export function AppImageMenuEntrySwitch() {
  const { t } = useTranslation("settings");
  const [on, setOn] = useState<boolean | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let active = true;
    getApi()
      .settingsGet()
      .then(
        (settings) => {
          if (active) setOn(settings.appimageMenuEntry === "yes");
        },
        (error: unknown) => {
          console.error("Loading the app menu choice failed", error);
        },
      );
    return () => {
      active = false;
    };
  }, []);

  if (on === null) return null;

  const change = async (add: boolean) => {
    setFailed(false);
    try {
      const api = getApi();
      if (add) await api.appimageMenuEntryAdd();
      const settings = await api.settingsGet();
      await api.settingsUpdate({ ...settings, appimageMenuEntry: add ? "yes" : "no" });
      setOn(add);
    } catch (error) {
      console.error("Changing the app menu entry failed", error);
      setFailed(true);
    }
  };

  return (
    <>
      <Switch
        label={t("appImageMenu.label")}
        description={t("appImageMenu.hint")}
        checked={on}
        onChange={(add) => {
          void change(add);
        }}
      />
      {failed && (
        <p role="alert" className="settings-error">
          {t("appImageMenu.failed")}
        </p>
      )}
    </>
  );
}
