import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../../api";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";
import { errorText } from "../errors";
import { pickOpenPath } from "../fileDialogs";
import type { SettingsSectionProps } from "./types";

const IMAGE_EXTENSIONS = ["png", "jpg", "jpeg", "gif", "webp"];

/**
 * Profile: the default avatar (preview, choose, clear) and whether it is sent automatically to
 * servers connected to for the first time (FR-1.47).
 */
export function DefaultAvatarGroup({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const [avatar, setAvatar] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    getApi()
      .defaultAvatarGet()
      .then(
        (loaded) => {
          if (active) setAvatar(loaded);
        },
        (e: unknown) => {
          console.error("Loading the default avatar failed", e);
          if (active) setError(errorText(t, e));
        },
      );
    return () => {
      active = false;
    };
  }, [t]);

  const store = async (path: string | null) => {
    setError(null);
    try {
      setAvatar(await getApi().defaultAvatarSet(path));
    } catch (e: unknown) {
      console.error("Changing the default avatar failed", e);
      setError(errorText(t, e));
    }
  };

  const choose = async () => {
    setError(null);
    try {
      const filter = { name: t("profile.imageType"), extensions: IMAGE_EXTENSIONS };
      const path = await pickOpenPath(filter, t("profile.chooseTitle"));
      if (path !== null) await store(path);
    } catch (e: unknown) {
      console.error("Choosing the default avatar failed", e);
      setError(errorText(t, e));
    }
  };

  return (
    <Group title={t("profile.title")}>
      <p className="settings-value">{t("profile.defaultAvatar")}</p>
      <div className="settings-avatar">
        {avatar !== null ? (
          <img className="settings-avatar__image" src={avatar} alt={t("profile.preview")} />
        ) : (
          <p className="settings-hint">{t("profile.none")}</p>
        )}
      </div>
      <div className="settings-actions">
        <button type="button" className="settings-button" onClick={() => void choose()}>
          {t("profile.choose")}
        </button>
        <button
          type="button"
          className="settings-button"
          disabled={avatar === null}
          onClick={() => void store(null)}
        >
          {t("profile.clear")}
        </button>
      </div>
      <Switch
        label={t("profile.sendToNew")}
        description={t("profile.sendToNewHint")}
        checked={settings.sendDefaultAvatar}
        onChange={(enabled) => {
          update({ sendDefaultAvatar: enabled });
        }}
      />
      {error !== null && (
        <p role="alert" className="settings-error">
          {error}
        </p>
      )}
    </Group>
  );
}
