import { useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../../api";
import { setDevToolsUnlocked } from "../../inspector/devTools";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";
import { errorText } from "../errors";

/** Hidden developer tools (issue #60): the switch that hides them and the inspector window. */
export function DeveloperToolsCard() {
  const { t } = useTranslation("settings");
  const [error, setError] = useState<string | null>(null);

  const open = () => {
    setError(null);
    getApi()
      .packetInspectorOpen()
      .catch((cause: unknown) => {
        console.error("Opening the packet inspector failed", cause);
        setError(errorText(t, cause));
      });
  };

  return (
    <Group title={t("inspector.title")} icon="code" info={t("inspector.intro")}>
      <Switch
        label={t("inspector.devTools")}
        description={t("inspector.devToolsHint")}
        checked
        onChange={(on) => {
          if (!on) setDevToolsUnlocked(false);
        }}
      />
      <div className="settings-actions">
        <button type="button" className="settings-button settings-button--primary" onClick={open}>
          {t("inspector.open")}
        </button>
      </div>
      <p role="status" className="settings-error">
        {error}
      </p>
    </Group>
  );
}
