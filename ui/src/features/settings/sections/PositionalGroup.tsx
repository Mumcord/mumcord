import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { BetaBadge } from "../../../components/BetaBadge";
import { usePositionalStatus } from "../../positional/usePositionalStatus";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";
import type { SettingsSectionProps } from "./types";

/** Largest distance the engine accepts, in metres. */
const MAX_DISTANCE = 1000;

/** Parses a distance field; `null` for empty or non-numeric text. */
function parse(text: string): number | null {
  if (text.trim() === "") return null;
  const value = Number(text);
  return Number.isFinite(value) ? value : null;
}

/** The engine's rule: 0 <= min, max >= min + 1, max <= 1000. */
function validDistances(min: number | null, max: number | null): boolean {
  return min !== null && max !== null && min >= 0 && max >= min + 1 && max <= MAX_DISTANCE;
}

/** What the status line says (FR-1.43). */
function useStatusLine(): string | null {
  const { t } = useTranslation("settings");
  const status = usePositionalStatus();
  if (status === null) return null;
  if (status.source === null) return t("positional.none");
  return status.game === null
    ? t("positional.source", { source: status.source })
    : t("positional.sourceGame", { source: status.source, game: status.game });
}

/**
 * Positional audio (FR-1.41, FR-1.43): on/off, the distance range and where positions come
 * from right now. The same switch is in the quick settings. The game plugins have their
 * own group next to this one.
 */
export function PositionalGroup({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const { enabled, minDistance, maxDistance } = settings.positional;
  const [min, setMin] = useState(String(minDistance ?? ""));
  const [max, setMax] = useState(String(maxDistance ?? ""));
  const minId = useId();
  const maxId = useId();
  const errorId = useId();
  const line = useStatusLine();
  const valid = validDistances(parse(min), parse(max));

  // Only a valid pair is saved; invalid text stays in the fields with an error beside it.
  const edit = (nextMin: string, nextMax: string) => {
    setMin(nextMin);
    setMax(nextMax);
    const low = parse(nextMin);
    const high = parse(nextMax);
    if (validDistances(low, high)) {
      update({ positional: { ...settings.positional, minDistance: low, maxDistance: high } });
    }
  };

  return (
    <Group
      title={t("positional.title")}
      icon="compass"
      badge={<BetaBadge />}
      info={t("positional.betaHint")}
    >
      <Switch
        label={t("positional.switch")}
        badge={<BetaBadge />}
        description={t("positional.hint")}
        checked={enabled}
        onChange={(on) => {
          update({ positional: { ...settings.positional, enabled: on } });
        }}
      />
      <div className="settings-field-row">
        <div className="settings-control-row">
          <label htmlFor={minId}>{t("positional.minDistance")}</label>
          <input
            id={minId}
            type="number"
            className="settings-input settings-input--compact"
            min={0}
            max={MAX_DISTANCE}
            step="any"
            value={min}
            aria-invalid={!valid}
            aria-describedby={valid ? undefined : errorId}
            onChange={(event) => {
              edit(event.target.value, max);
            }}
          />
        </div>
        <div className="settings-control-row">
          <label htmlFor={maxId}>{t("positional.maxDistance")}</label>
          <input
            id={maxId}
            type="number"
            className="settings-input settings-input--compact"
            min={1}
            max={MAX_DISTANCE}
            step="any"
            value={max}
            aria-invalid={!valid}
            aria-describedby={valid ? undefined : errorId}
            onChange={(event) => {
              edit(min, event.target.value);
            }}
          />
        </div>
      </div>
      {!valid && (
        <p id={errorId} role="alert" className="settings-error">
          {t("positional.invalid")}
        </p>
      )}
      {line !== null && (
        <p className="settings-hint" role="status">
          {line}
        </p>
      )}
    </Group>
  );
}
