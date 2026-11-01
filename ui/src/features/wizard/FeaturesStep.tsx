import { useTranslation } from "react-i18next";
import ingameOverlayExample from "../../assets/ingame-overlay-example.png";
import { Switch } from "../settings/components/Switch";
import type { SettingsSectionProps } from "../settings/sections/types";
import { StepFrame } from "./StepFrame";

interface FeaturesStepProps extends SettingsSectionProps {
  /** In-game overlay on; `null` where this platform cannot draw inside games (card hidden). */
  ingameOverlay: boolean | null;
  dndFollowSystem: boolean;
  onIngameOverlayChange: (on: boolean) => void;
  onDndFollowSystemChange: (on: boolean) => void;
  onNext: () => void;
}

/**
 * "Make it yours" (Custom only, optional): now playing, now gaming, the in-game overlay and
 * "Follow system Do Not Disturb", each with who sees what behind its "?".
 */
export function FeaturesStep({
  settings,
  update,
  ingameOverlay,
  dndFollowSystem,
  onIngameOverlayChange,
  onDndFollowSystemChange,
  onNext,
}: FeaturesStepProps) {
  const { t } = useTranslation("wizard");
  const { t: ts } = useTranslation("settings");
  const tip = (label: string, text: string) => ({ label: t("tipLabel", { name: label }), text });
  return (
    <StepFrame title={t("features.title")} subtitle={t("features.subtitle")} onSubmit={onNext}>
      <div className="wizard-grid">
        <section className="wizard-card" aria-label={ts("nowPlaying.title")}>
          <Switch
            label={ts("nowPlaying.switch")}
            tip={tip(ts("nowPlaying.switch"), ts("nowPlaying.hint"))}
            checked={settings.nowPlaying}
            onChange={(nowPlaying) => {
              update({ nowPlaying });
            }}
          />
          <p className="wizard-muted wizard-small">{t("features.seenByMumcordUsers")}</p>
        </section>
        <section className="wizard-card" aria-label={ts("nowGaming.title")}>
          <Switch
            label={ts("nowGaming.switch")}
            tip={tip(ts("nowGaming.switch"), ts("nowGaming.hint"))}
            checked={settings.nowGaming}
            onChange={(nowGaming) => {
              update({ nowGaming });
            }}
          />
          <p className="wizard-muted wizard-small">{t("features.seenByMumcordUsers")}</p>
        </section>
        {ingameOverlay !== null && (
          <section className="wizard-card" aria-label={ts("overlay.inGame.title")}>
            <Switch
              label={ts("overlay.inGame.title")}
              tip={tip(ts("overlay.inGame.title"), ts("overlay.inGame.explanation"))}
              checked={ingameOverlay}
              onChange={onIngameOverlayChange}
            />
            <img
              className="wizard-overlay-example"
              src={ingameOverlayExample}
              alt={t("features.ingameOverlayAlt")}
              width={360}
              height={210}
            />
            <p className="wizard-muted wizard-small">{t("features.onlyYou")}</p>
            <p className="wizard-note wizard-small">{ts("overlay.inGame.antiCheat")}</p>
          </section>
        )}
        <section className="wizard-card" aria-label={ts("dnd.title")}>
          <Switch
            label={ts("dnd.follow")}
            badge={t("recommended")}
            tip={tip(ts("dnd.follow"), ts("dnd.followDescription"))}
            checked={dndFollowSystem}
            onChange={onDndFollowSystemChange}
          />
        </section>
      </div>
    </StepFrame>
  );
}
