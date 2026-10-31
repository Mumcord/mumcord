import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { InfoTip } from "../../../components/InfoTip";
import { Group } from "../components/Section";
import type { SettingsSectionProps } from "./types";

const MAX_SERVERS_RANGE = { min: 1, max: 15 } as const;

function parseLimit(text: string): number | null {
  if (!/^\d+$/.test(text)) return null;
  const value = Number(text);
  return value >= MAX_SERVERS_RANGE.min && value <= MAX_SERVERS_RANGE.max ? value : null;
}

/** How many servers may be connected at the same time (FR-2.1). */
export function ServerConnectionsCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const id = useId();
  const errorId = `${id}-error`;
  const [draft, setDraft] = useState(String(settings.maxConnectedServers));
  const valid = parseLimit(draft) !== null;
  return (
    <Group title={t("servers.connections")} icon="server">
      <div className="settings-control-row">
        <span className="settings-control-row__label">
          <label htmlFor={id}>{t("servers.limit")}</label>
          <InfoTip
            label={t("moreInfo", { topic: t("servers.limit") })}
            text={t("servers.limitHint")}
          />
        </span>
        <input
          id={id}
          type="number"
          inputMode="numeric"
          className="settings-input settings-input--compact"
          min={MAX_SERVERS_RANGE.min}
          max={MAX_SERVERS_RANGE.max}
          step={1}
          value={draft}
          aria-invalid={!valid}
          aria-describedby={valid ? undefined : errorId}
          onChange={(event) => {
            setDraft(event.target.value);
            const limit = parseLimit(event.target.value);
            if (limit !== null) update({ maxConnectedServers: limit });
          }}
        />
      </div>
      {!valid && (
        <p id={errorId} className="settings-error" role="alert">
          {t("servers.limitInvalid")}
        </p>
      )}
    </Group>
  );
}
