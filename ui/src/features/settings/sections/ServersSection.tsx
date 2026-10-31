import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { SettingsImport } from "../../mumbleImport/SettingsImport";
import { Group, Section } from "../components/Section";
import type { SettingsSectionProps } from "./types";

const MAX_SERVERS_RANGE = { min: 1, max: 15 } as const;

function parseLimit(text: string): number | null {
  if (!/^\d+$/.test(text)) return null;
  const value = Number(text);
  return value >= MAX_SERVERS_RANGE.min && value <= MAX_SERVERS_RANGE.max ? value : null;
}

/**
 * Servers: how many servers may be connected at the same time (FR-2.1) and the import from the
 * official Mumble client (FR-3.94).
 */
export function ServersSection({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const id = useId();
  const hintId = `${id}-hint`;
  const [draft, setDraft] = useState(String(settings.maxConnectedServers));
  const valid = parseLimit(draft) !== null;
  return (
    <Section title={t("sections.servers")}>
      <Group title={t("servers.connections")}>
        <div className="settings-control-row">
          <label htmlFor={id}>{t("servers.limit")}</label>
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
            aria-describedby={hintId}
            onChange={(event) => {
              setDraft(event.target.value);
              const limit = parseLimit(event.target.value);
              if (limit !== null) update({ maxConnectedServers: limit });
            }}
          />
        </div>
        <p
          id={hintId}
          className={valid ? "settings-hint" : "settings-error"}
          role={valid ? undefined : "alert"}
        >
          {valid ? t("servers.limitHint") : t("servers.limitInvalid")}
        </p>
      </Group>
      <SettingsImport settings={settings} update={update} />
    </Section>
  );
}
