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
import { InstallProgress } from "./InstallProgress";
import { clearInstallProgress } from "./installProgressStore";
import { showPluginToast } from "./installToast";
import "./prompt.css";

type Phase =
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
  /** First install from this third-party repository: the user must confirm the warning. */
  repositoryWarning: boolean;
  suggestedBy?: SuggestedBy | null;
  onClose: () => void;
}

/**
 * The install prompt (spec §5.3, FR-6.60). The engine first downloads and verifies the package
 * so the prompt can list its required and optional permissions; nothing runs until Install.
 */
export function InstallPrompt({
  plugin,
  repositoryWarning,
  suggestedBy = null,
  onClose,
}: InstallPromptProps) {
  const { t } = useTranslation("pluginBrowse");
  const { servers } = useServers();
  const serverName = servers.find((s) => s.id === suggestedBy?.serverId)?.label ?? null;
  const [phase, setPhase] = useState<Phase>({ kind: "preparing" });
  const [enabled, setEnabled] = useState<ReadonlySet<string>>(new Set());
  const [visibility, setVisibility] = useState<PluginVisibilityChoice>("everyone");
  const [understood, setUnderstood] = useState(false);
  // Bumped by "Try again": downloads and checks the package once more.
  const [attempt, setAttempt] = useState(0);
  const visibilityName = useId();
  const name = plugin.name;

  useEffect(() => {
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
  }, [plugin.id, name, t, attempt]);

  const installing = phase.kind === "installing";

  const install = (offer: PluginInstallOffer) => {
    setPhase({ kind: "installing", offer });
    getApi()
      .pluginInstall(plugin.id, offer.version, [...enabled], visibility)
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
      icon={<PluginIcon id={plugin.id} name={name} hasIcon={plugin.icon !== null} />}
      subtitle={
        <p className="plugin-prompt-source">
          {sourceLabel(t, plugin.source)}
          {phase.kind === "ready" || phase.kind === "installing" ? ` · ${phase.offer.version}` : ""}
        </p>
      }
    >
      {phase.kind === "preparing" && (
        <p role="status" className="plugin-prompt-status">
          {t("prompt.preparing", { name })}
        </p>
      )}
      {(phase.kind === "preparing" || installing) && <InstallProgress id={plugin.id} name={name} />}
      {phase.kind === "failed" && (
        <p role="alert" className="settings-error">
          {phase.message}
        </p>
      )}
      {(phase.kind === "ready" || phase.kind === "installing") && (
        <>
          {suggestedBy !== null && (
            <div className="plugin-prompt-box" role="note">
              <strong>
                {t("prompt.suggestedTitle", { server: serverName ?? t("prompt.thisServer") })}
              </strong>
              {suggestedBy.reason !== "" && <p>{suggestedBy.reason}</p>}
              <p>{t("prompt.suggestedText")}</p>
            </div>
          )}
          <p className="plugin-prompt-subtitle">{t("prompt.intro", { name })}</p>
          <PermissionList
            requested={requestedOf(phase.offer)}
            chatPatterns={phase.offer.chatPatterns}
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
          {asksFor(phase.offer, MESSAGES) && (
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
          {repositoryWarning && plugin.source.kind === "repo" && (
            <div
              className="plugin-prompt-warning"
              role="group"
              aria-labelledby={`${visibilityName}-warning`}
            >
              <strong id={`${visibilityName}-warning`}>{t("prompt.warningTitle")}</strong>
              <p>{t("prompt.warningText", { repository: plugin.source.name })}</p>
              <label>
                <input
                  type="checkbox"
                  checked={understood}
                  disabled={installing}
                  onChange={(event) => {
                    setUnderstood(event.target.checked);
                  }}
                />{" "}
                {t("prompt.understand")}
              </label>
            </div>
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
        {(phase.kind === "ready" || phase.kind === "installing") && (
          <button
            type="button"
            className="settings-button settings-button--primary"
            disabled={
              installing || (repositoryWarning && plugin.source.kind === "repo" && !understood)
            }
            onClick={() => {
              if (phase.kind === "ready") install(phase.offer);
            }}
          >
            {t("prompt.install")}
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
