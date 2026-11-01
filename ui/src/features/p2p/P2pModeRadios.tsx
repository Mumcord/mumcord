import { useId } from "react";
import { useTranslation } from "react-i18next";
import type { P2pMode } from "../../api";
import { useIsChanged } from "../settings/components/changedSettings";
import { ChangedBadge, RecommendedBadge } from "../settings/components/SettingBadges";
import "./p2p.css";

/** The five modes in the order of FR-4.1; the first one is the default. */
const P2P_MODES: readonly P2pMode[] = [
  "directRelay",
  "relayFirst",
  "directOnly",
  "relayOnly",
  "off",
];

const RECOMMENDED: P2pMode = "directRelay";
const GLOBAL = "global";

interface P2pModeRadiosProps {
  /** `null` is "Use global setting"; only valid together with `allowGlobal`. */
  value: P2pMode | null;
  onChange: (mode: P2pMode | null) => void;
  /** Adds "Use global setting" as the first option (per-server override, FR-4.2). */
  allowGlobal?: boolean;
  /** Settings already show the title as the group heading. */
  hideTitle?: boolean;
}

/**
 * The peer-to-peer mode (FR-4.1) as native radio buttons, shared by Settings → Privacy, the
 * setup wizard and the server dialog. Arrow keys move between the options; the group is named
 * "Peer-to-peer mode" for screen readers and described by the explanation text.
 */
export function P2pModeRadios({
  value,
  onChange,
  allowGlobal = false,
  hideTitle = false,
}: P2pModeRadiosProps) {
  const { t } = useTranslation("settings");
  const name = useId();
  const explanationId = useId();
  const options: (P2pMode | typeof GLOBAL)[] = allowGlobal
    ? [GLOBAL, ...P2P_MODES]
    : [...P2P_MODES];
  const selected = value ?? GLOBAL;
  const changed = useIsChanged(t("p2p.title"));
  return (
    <fieldset
      className="p2p-modes"
      aria-label={changed ? `${t("p2p.modeLabel")}, ${t("badges.changed")}` : t("p2p.modeLabel")}
      aria-describedby={explanationId}
    >
      <legend className={hideTitle ? "visually-hidden" : "p2p-modes__legend"}>
        {t("p2p.title")}
      </legend>
      {/* Only inside Settings: there it marks a mode that differs from the preset (#161). */}
      <span className="setting-badges" aria-hidden="true">
        <ChangedBadge label={t("p2p.title")} />
      </span>
      {options.map((option) => (
        <label key={option} className="p2p-modes__option">
          <input
            type="radio"
            name={name}
            value={option}
            checked={option === selected}
            onChange={() => {
              onChange(option === GLOBAL ? null : option);
            }}
          />
          {option === GLOBAL ? t("p2p.useGlobal") : t(`p2p.mode.${option}`)}
          {option === RECOMMENDED && (
            <>
              {" "}
              <RecommendedBadge className="p2p-modes__badge" />
            </>
          )}
        </label>
      ))}
      <p id={explanationId} className="p2p-modes__explanation">
        {t("p2p.explanation")}
        {allowGlobal && ` ${t("p2p.overrideHint")}`}
      </p>
    </fieldset>
  );
}
