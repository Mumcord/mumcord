import { useTranslation } from "react-i18next";
import type { Settings } from "../../api";
import { P2pModeRadios } from "../p2p/P2pModeRadios";
import { Switch } from "../settings/components/Switch";
import type { SettingsSectionProps } from "../settings/sections/types";
import { StepFrame } from "./StepFrame";

type ContentKey = "linkPreviews" | "offerFullImages" | "loadFullImages" | "historySharing";

/** Settings keys and the Settings label / hint keys of each switch (same text as in Settings). */
const CHAT_CONTENT: readonly [ContentKey, string, string][] = [
  ["linkPreviews", "linkPreviews.switch", "linkPreviews.hint"],
];
const SHARING: readonly [ContentKey, string, string][] = [
  ["offerFullImages", "fullImages.offer", "fullImages.offerHint"],
  ["loadFullImages", "fullImages.load", "fullImages.loadHint"],
  ["historySharing", "historySharing.switch", "historySharing.hint"],
];

/**
 * "Connections and content" (Custom only): the peer-to-peer mode (#287), link previews,
 * full-size images and history sync, with the controls and keys of Settings → Privacy.
 */
export function ConnectionsStep({
  settings,
  update,
  onNext,
}: SettingsSectionProps & { onNext: () => void }) {
  const { t } = useTranslation("wizard");
  const { t: ts } = useTranslation("settings");

  function rows(list: readonly [ContentKey, string, string][]) {
    return list.map(([key, label, hint]) => (
      <Switch
        key={key}
        label={ts(label)}
        tip={{ label: t("tipLabel", { name: ts(label) }), text: ts(hint) }}
        checked={settings[key]}
        onChange={(on) => {
          const patch: Partial<Settings> = {};
          patch[key] = on;
          update(patch);
        }}
      />
    ));
  }

  return (
    <StepFrame
      title={t("connections.title")}
      subtitle={t("connections.subtitle")}
      onSubmit={onNext}
    >
      <div className="wizard-columns">
        <section className="wizard-card" aria-label={ts("p2p.modeLabel")}>
          <P2pModeRadios
            value={settings.p2pMode}
            onChange={(p2pMode) => {
              // Without `allowGlobal` the radios never offer "Use global setting".
              if (p2pMode !== null) update({ p2pMode });
            }}
          />
        </section>
        <div className="wizard-column">
          <h3 className="wizard-section-title">{t("connections.chatContent")}</h3>
          <section className="wizard-card wizard-rows" aria-label={t("connections.chatContent")}>
            {rows(CHAT_CONTENT)}
          </section>
          <h3 className="wizard-section-title">{t("connections.sharing")}</h3>
          <section className="wizard-card wizard-rows" aria-label={t("connections.sharing")}>
            {rows(SHARING)}
          </section>
        </div>
      </div>
    </StepFrame>
  );
}
