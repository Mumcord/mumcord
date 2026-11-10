import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  getApi,
  type IndexedPlugin,
  type PluginInstallOffer,
  type PluginVisibilityChoice,
} from "../../../api";
import { useServers } from "../../servers";
import { ModalDialog } from "../../settings/components/ModalDialog";
import { defaultOptionalSelection, type RequestedPermission } from "../permissions/catalogue";
import { PermissionList } from "../permissions/PermissionList";
import { PluginIcon } from "../browse/PluginIcon";
import { sourceLabel } from "../browse/source";
import { pluginErrorText } from "./errorText";
import { ScoreChip } from "../score";
import { AllowAll } from "./AllowAll";
import { ConfirmBox } from "./ConfirmBox";
import { hasNewAuthorKey } from "./highRisk";
import { InstallProgress } from "./InstallProgress";
import { clearInstallProgress } from "./installProgressStore";
import { showPluginToast } from "./installToast";
import { ScoreSummary } from "./ScoreSummary";
import { usePromptRowDetails, type PromptOrigin } from "./usePromptRowDetails";
import "./prompt.css";

type Phase =
  /** No version runs on this Mumcord (FR-9.16): nothing is downloaded, Install stays disabled. */
  | { kind: "needsMumcord"; version: string }
  | { kind: "preparing" }
  | { kind: "ready"; offer: PluginInstallOffer }
  | { kind: "installing"; offer: PluginInstallOffer }
  | { kind: "failed"; message: string };

const VISIBILITIES: readonly PluginVisibilityChoice[] = ["everyone", "friends", "nobody"];
const MESSAGES = "peers.messages";

/** The server suggestion an install came from (the notification centre), if any. */
export interface SuggestedBy {
  serverId: number;
  /** Why the server suggests it; written by the server, so untrusted text. */
  reason: string;
}

export interface InstallPromptProps {
  plugin: IndexedPlugin;
  suggestedBy?: SuggestedBy | null;
  onClose: () => void;
}

/**
 * The install prompt (spec 06 §5.3, spec 09 §5.2). The engine first downloads and verifies the
 * package so the prompt can list its permissions, score and gate; nothing runs until Install.
 */
export function InstallPrompt({ plugin, suggestedBy = null, onClose }: InstallPromptProps) {
  const { t } = useTranslation("pluginBrowse");
  const { t: tGate } = useTranslation("pluginGate");
  const { t: tScore } = useTranslation("pluginScore");
  const { t: tApi } = useTranslation("pluginApi");
  const { servers } = useServers();
  const serverName = servers.find((s) => s.id === suggestedBy?.serverId)?.label ?? null;
  const [phase, setPhase] = useState<Phase>(
    plugin.needsMumcord === null
      ? { kind: "preparing" }
      : { kind: "needsMumcord", version: plugin.needsMumcord },
  );
  const [enabled, setEnabled] = useState<ReadonlySet<string>>(new Set());
  const [visibility, setVisibility] = useState<PluginVisibilityChoice>("everyone");
  // The one confirmation box (FR-9.80).
  const [ticked, setTicked] = useState(false);
  // Bumped by "Try again": downloads and checks the package once more.
  const [attempt, setAttempt] = useState(0);
  const visibilityName = useId();
  const name = plugin.name;

  const needsMumcord = plugin.needsMumcord !== null;

  useEffect(() => {
    if (needsMumcord) return;
    let cancelled = false;
    getApi()
      .pluginInstallPrepare(plugin.id)
      .then((offer) => {
        clearInstallProgress(plugin.id);
        if (cancelled) return;
        setEnabled(defaultOptionalSelection(requestedOf(offer)));
        setVisibility(offer.defaultVisibility);
        setPhase({ kind: "ready", offer });
      })
      .catch((error: unknown) => {
        console.error("Preparing the plugin install failed", error);
        clearInstallProgress(plugin.id);
        if (!cancelled) {
          setPhase({
            kind: "failed",
            message: pluginErrorText(t, error, t("prompt.failed", { name })),
          });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [plugin.id, name, t, attempt, needsMumcord]);

  const installing = phase.kind === "installing";
  const offer = phase.kind === "ready" || phase.kind === "installing" ? phase.offer : null;
  const check = offer?.check ?? null;
  const gate = check?.kind === "ready" ? check.gate : null;
  const repository = plugin.source.kind === "repo" ? plugin.source.name : null;
  const permissions = offer === null ? [] : requestedOf(offer).map((p) => p.id);
  // The verified package's websites with their purposes; until it is checked, the listing's.
  const origins: PromptOrigin[] =
    offer === null ? plugin.origins.map((origin) => ({ origin, purpose: null })) : offer.origins;
  const rowDetails = usePromptRowDetails({ pluginName: name, repository, permissions, origins });
  const needsTick = gate !== null && gate.confirm !== null && !gate.blockedBySafety;
  const canInstall =
    phase.kind === "ready" && gate !== null && !gate.blockedBySafety && (!needsTick || ticked);

  const install = (offer: PluginInstallOffer) => {
    setPhase({ kind: "installing", offer });
    getApi()
      .pluginInstall(plugin.id, offer.version, [...enabled], visibility, !needsTick || ticked)
      .then(() => {
        showPluginToast(t("toast.installed", { name }));
        if (suggestedBy) {
          getApi()
            .pluginSuggestionInstalled(suggestedBy.serverId, plugin.id)
            .catch((error: unknown) => {
              console.error("Remembering the installed suggestion failed", error);
            });
        }
        onClose();
      })
      .catch((error: unknown) => {
        console.error("Installing the plugin failed", error);
        setPhase({
          kind: "failed",
          message: pluginErrorText(t, error, t("prompt.failed", { name })),
        });
      });
  };

  // The install cannot be stopped once it runs; the dialog stays until it finished.
  const close = () => {
    if (!installing) onClose();
  };

  return (
    <ModalDialog
      title={t("prompt.title", { name })}
      onClose={close}
      className="plugin-prompt"
      focusTitle
      icon={<PluginIcon id={plugin.id} name={name} hasIcon={plugin.icon !== null} />}
      subtitle={
        <p className="plugin-prompt-source">
          {plugin.source.kind === "official" ? (
            sourceLabel(t, plugin.source)
          ) : (
            <span className="plugin-prompt-chip plugin-prompt-chip--unofficial">
              {sourceLabel(t, plugin.source)}
            </span>
          )}
          {offer === null ? "" : ` · ${offer.version}`}
          {offer !== null &&
            offer.authors.length > 0 &&
            ` · ${t("by", { authors: offer.authors.join(", ") })}`}
        </p>
      }
    >
      {phase.kind === "preparing" && (
        <p role="status" className="plugin-prompt-status">
          {t("prompt.preparing", { name })}
        </p>
      )}
      {(phase.kind === "preparing" || installing) && <InstallProgress id={plugin.id} name={name} />}
      {phase.kind === "needsMumcord" && (
        <p role="status" className="plugin-prompt-notice">
          {tApi("tooNew", { version: phase.version })}
        </p>
      )}
      {check?.kind === "refused" && (
        <div className="plugin-prompt-refused" role="alert">
          <ScoreChip scored={{ kind: "refused", stop: check.stop }} size="md" />
          <p>{check.reason}</p>
        </div>
      )}
      {phase.kind === "failed" && (
        <p role="alert" className="settings-error">
          {phase.message}
        </p>
      )}
      {offer !== null && check?.kind === "ready" && (
        <>
          <ScoreSummary
            score={check.score}
            pluginName={name}
            blocked={check.gate.blockedBySafety}
            websites={origins.length}
          />
          {suggestedBy !== null && (
            <div className="plugin-prompt-box" role="note">
              <strong>
                {t("prompt.suggestedTitle", { server: serverName ?? t("prompt.thisServer") })}
              </strong>
              {suggestedBy.reason !== "" && <p>{suggestedBy.reason}</p>}
              <p>{t("prompt.suggestedText")}</p>
            </div>
          )}
          <AllowAll
            pluginName={name}
            optional={offer.optional.map((o) => o.id)}
            enabled={enabled}
            disabled={installing}
            onAllow={(ids) => {
              setEnabled((current) => new Set([...current, ...ids]));
            }}
          />
          <PermissionList
            requested={requestedOf(offer)}
            pluginName={plugin.name}
            chatPatterns={offer.chatPatterns}
            details={rowDetails}
            enabled={enabled}
            readOnly={installing}
            onToggle={(id, on) => {
              setEnabled((current) => {
                const next = new Set(current);
                if (on) next.add(id);
                else next.delete(id);
                return next;
              });
            }}
          />
          {asksFor(offer, MESSAGES) && (
            <div className="plugin-prompt-visibility">
              <div className="plugin-prompt-visibility-text">
                <span id={`${visibilityName}-label`} className="plugin-prompt-visibility-title">
                  {t("prompt.visibilityTitle", { name })}
                </span>
                <span className="plugin-prompt-visibility-hint">
                  {t(`prompt.visibilityHint${capitalize(visibility)}`)}
                </span>
              </div>
              <div
                className="plugin-prompt-segments"
                role="radiogroup"
                aria-labelledby={`${visibilityName}-label`}
              >
                {VISIBILITIES.map((choice) => (
                  <label key={choice} className="plugin-prompt-segment">
                    <input
                      type="radio"
                      name={visibilityName}
                      value={choice}
                      checked={visibility === choice}
                      disabled={installing}
                      onChange={() => {
                        setVisibility(choice);
                      }}
                    />
                    <span>{t(`prompt.visibility${capitalize(choice)}`)}</span>
                  </label>
                ))}
              </div>
            </div>
          )}
          {plugin.source.kind === "official" && (
            <div className="plugin-prompt-box" role="note">
              <strong>{t("prompt.neverTitle")}</strong>
              <p>{t("neverText")}</p>
            </div>
          )}
          {needsTick && (
            <ConfirmBox
              confirm={check.gate.confirm ?? "understand"}
              reasons={check.gate.reasons}
              pluginName={name}
              repository={repository}
              highRisk={{
                webPages: permissions.includes("ui.webview"),
                websites: origins.length,
                newAuthor:
                  check.score.score.kind === "scored" &&
                  hasNewAuthorKey(check.score.score.score.factors),
              }}
              checked={ticked}
              disabled={installing}
              onChange={setTicked}
            />
          )}
          {check.score.score.kind === "scored" && (
            <p className="plugin-prompt-honesty">
              {tGate("scoreCountsOptional")} {tScore("honestyShort")}
            </p>
          )}
        </>
      )}
      <div className="settings-dialog__actions plugin-prompt-actions">
        <span className="plugin-prompt-footnote">{t("prompt.footnote")}</span>
        <button type="button" className="settings-button" disabled={installing} onClick={close}>
          {phase.kind === "failed" ? t("prompt.close") : t("prompt.cancel")}
        </button>
        {phase.kind === "failed" && (
          <button
            type="button"
            className="settings-button settings-button--primary"
            onClick={() => {
              setPhase({ kind: "preparing" });
              setAttempt((n) => n + 1);
            }}
          >
            {t("prompt.retry")}
          </button>
        )}
        {(phase.kind === "needsMumcord" || check?.kind === "ready") && (
          <button
            type="button"
            className="settings-button settings-button--primary"
            disabled={!canInstall}
            onClick={() => {
              if (phase.kind === "ready" && canInstall) install(phase.offer);
            }}
          >
            {gate?.confirm === "highRisk" && !gate.blockedBySafety
              ? tGate("installAnyway")
              : t("prompt.install")}
          </button>
        )}
      </div>
    </ModalDialog>
  );
}

function requestedOf(offer: PluginInstallOffer): RequestedPermission[] {
  return [
    ...offer.required.map((id) => ({ id, required: true })),
    ...offer.optional.map((o) => ({ id: o.id, required: false, suggested: o.suggested })),
  ];
}

function asksFor(offer: PluginInstallOffer, id: string): boolean {
  return offer.required.includes(id) || offer.optional.some((o) => o.id === id);
}

function capitalize(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}
