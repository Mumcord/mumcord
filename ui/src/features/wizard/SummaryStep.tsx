import { useTranslation } from "react-i18next";
import type { Identity } from "../../api";
import { findTheme, fromStoredTheme, selectedAppThemeId } from "../../theme/appTheme";
import { updateModeOf } from "../updates/updateMode";
import { StepFrame } from "./StepFrame";
import { CheckIcon } from "./wizardIcons";
import type { WizardDraft } from "./wizardDraft";
import type { WizardStepId } from "./wizardFlow";

interface SummaryStepProps {
  draft: WizardDraft;
  /** First start: the identity card and the backup reminder; `null` in the rerun. */
  identity: Identity | null;
  /** The identity name for the greeting, also in the rerun. */
  name: string | null;
  firstStart: boolean;
  busy: boolean;
  error: string | null;
  onEdit: (step: WizardStepId) => void;
  onFinish: () => void;
}

interface Card {
  id: string;
  step: WizardStepId;
  title: string;
  detail: string;
}

/**
 * "You're all set": every choice as a card with Edit, the identity backup reminder and
 * "Start Mumcord" (FR-3.115). Edit opens the step; its Next or Back comes back here.
 */
export function SummaryStep({
  draft,
  identity,
  name,
  firstStart,
  busy,
  error,
  onEdit,
  onFinish,
}: SummaryStepProps) {
  const { t } = useTranslation("wizard");
  const { t: ts } = useTranslation("settings");
  const { settings } = draft;
  const onOff = (on: boolean) => (on ? t("summary.on") : t("summary.off"));
  const pm = settings.privateMessages;
  const customThemes = settings.customThemes.map(fromStoredTheme);
  const theme = findTheme(selectedAppThemeId(settings.appThemeId, customThemes), customThemes);
  const noise = settings.noiseSuppression ? settings.noiseSuppressionBackend : "off";

  const cards: Card[] = [
    ...(identity === null
      ? []
      : [
          {
            id: "identity",
            step: "identity" as const,
            title: identity.name,
            detail: t("summary.identityDetail"),
          },
        ]),
    {
      id: "voice",
      step: "voice",
      title: ts(`voice.${settings.talkMode}`),
      detail: t("summary.voiceDetail", { noise: ts(`voice.noiseChoice.${noise}`) }),
    },
    {
      id: "privateMessages",
      step: "privateMessages",
      title: ts(`privateMessages.${pm.rule}`),
      detail: t("summary.pmDetail", {
        channel: onOff(pm.allowCurrentChannel),
        accepted: onOff(pm.allowAccepted),
        requests: onOff(pm.keepRequests),
      }),
    },
    {
      id: "connections",
      step: "connections",
      title: ts(`p2p.mode.${settings.p2pMode}`),
      detail: ts("p2p.modeLabel"),
    },
    {
      id: "chatContent",
      step: "connections",
      title: t("summary.contentTitle", { previews: onOff(settings.linkPreviews) }),
      detail: t("summary.contentDetail", {
        images: onOff(settings.offerFullImages && settings.loadFullImages),
        sync: onOff(settings.historySharing),
      }),
    },
    {
      id: "features",
      step: "features",
      title: t("summary.featuresTitle", {
        overlay:
          draft.ingameOverlay === null ? t("summary.unavailable") : onOff(draft.ingameOverlay),
      }),
      detail: t("summary.featuresDetail", {
        dnd: onOff(draft.dndFollowSystem),
        playing: onOff(settings.nowPlaying),
        gaming: onOff(settings.nowGaming),
      }),
    },
    {
      id: "appearance",
      step: "appearance",
      title: `${theme?.name ?? settings.appThemeId} · ${ts(`appearance.${settings.theme}`)}`,
      detail: t("summary.effectsDetail", { effects: onOff(settings.visualEffects) }),
    },
    {
      id: "startup",
      step: "startup",
      title: t("summary.startupTitle", { login: onOff(settings.autostart) }),
      detail: t("summary.startupDetail", {
        minimized: onOff(settings.autostart && settings.startMinimized),
        links: onOff(draft.registerLinkHandler),
      }),
    },
    {
      id: "updates",
      step: "startup",
      title: t(`summary.updates.${updateModeOf(settings)}`),
      detail: t("summary.updatesDetail"),
    },
  ];

  return (
    <StepFrame
      title={name === null ? t("summary.title") : t("summary.titleNamed", { name })}
      subtitle={t("summary.subtitle")}
      busy={busy}
      onSubmit={onFinish}
      badge={
        <span className="wizard-summary__badge" aria-hidden="true">
          <CheckIcon />
        </span>
      }
      footerNote={
        firstStart ? <p className="wizard-muted wizard-small">{t("summary.nextServer")}</p> : null
      }
    >
      <ul className="wizard-grid wizard-grid--three">
        {cards.map((card) => (
          <li key={card.id} className="wizard-card wizard-summary-card">
            <div className="wizard-summary-card__head">
              <h3 className="wizard-summary-card__label">{t(`summary.cards.${card.id}`)}</h3>
              <button
                type="button"
                className="wizard-link"
                aria-label={t("summary.edit", { card: t(`summary.cards.${card.id}`) })}
                onClick={() => {
                  onEdit(card.step);
                }}
              >
                {t("summary.editShort")}
              </button>
            </div>
            <p className="wizard-summary-card__value">{card.title}</p>
            <p className="wizard-muted wizard-small">{card.detail}</p>
          </li>
        ))}
      </ul>
      {identity !== null && (
        <p className="wizard-note">{t("summary.backup", { name: identity.name })}</p>
      )}
      {!firstStart && <p className="wizard-muted">{t("summary.rerunNote")}</p>}
      {error && (
        <p role="alert" className="wizard-error">
          {error}
        </p>
      )}
    </StepFrame>
  );
}
