import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  getApi,
  type IndexedPlugin,
  type PluginInstallOffer,
  type PluginInstallStage,
  type PluginVisibilityChoice,
} from "../../../api";
import { ModalDialog } from "../../settings/components/ModalDialog";
import { defaultOptionalSelection, type RequestedPermission } from "../permissions/catalogue";
import { PermissionList } from "../permissions/PermissionList";
import { sourceLabel } from "../browse/source";
import { pluginErrorText } from "./errorText";
import { showPluginToast } from "./installToast";
import "./prompt.css";

type Phase =
  | { kind: "preparing" }
  | { kind: "ready"; offer: PluginInstallOffer }
  | { kind: "installing"; offer: PluginInstallOffer; stage: PluginInstallStage["stage"] | null }
  | { kind: "failed"; message: string };

const VISIBILITIES: readonly PluginVisibilityChoice[] = ["everyone", "friends", "nobody"];
const MESSAGES = "peers.messages";

export interface InstallPromptProps {
  plugin: IndexedPlugin;
  /** First install from this third-party repository: the user must confirm the warning. */
  repositoryWarning: boolean;
  onClose: () => void;
}

/**
 * The install prompt (spec §5.3, FR-6.60). The engine first downloads and verifies the package
 * so the prompt can list its required and optional permissions; nothing runs until Install.
 */
export function InstallPrompt({ plugin, repositoryWarning, onClose }: InstallPromptProps) {
  const { t } = useTranslation("pluginBrowse");
  const [phase, setPhase] = useState<Phase>({ kind: "preparing" });
  const [enabled, setEnabled] = useState<ReadonlySet<string>>(new Set());
  const [visibility, setVisibility] = useState<PluginVisibilityChoice>("everyone");
  const [understood, setUnderstood] = useState(false);
  const visibilityName = useId();
  const name = plugin.name;

  useEffect(() => {
    let cancelled = false;
    getApi()
      .pluginInstallPrepare(plugin.id)
      .then((offer) => {
        if (cancelled) return;
        setEnabled(defaultOptionalSelection(requestedOf(offer)));
        setVisibility(offer.defaultVisibility);
        setPhase({ kind: "ready", offer });
      })
      .catch((error: unknown) => {
        console.error("Preparing the plugin install failed", error);
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
  }, [plugin.id, name, t]);

  const installing = phase.kind === "installing";
  useEffect(() => {
    if (!installing) return;
    return getApi().on("pluginsInstallProgress", ({ id, progress }) => {
      if (id !== plugin.id) return;
      setPhase((current) =>
        current.kind === "installing" ? { ...current, stage: progress.stage } : current,
      );
    });
  }, [installing, plugin.id]);

  const install = (offer: PluginInstallOffer) => {
    setPhase({ kind: "installing", offer, stage: null });
    getApi()
      .pluginInstall(plugin.id, offer.version, [...enabled], visibility)
      .then(() => {
        showPluginToast(t("toast.installed", { name }));
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
    <ModalDialog title={t("prompt.title", { name })} onClose={close} className="plugin-prompt">
      <p className="plugin-prompt-source">{sourceLabel(t, plugin.source)}</p>
      {phase.kind === "preparing" && (
        <p role="status" className="plugin-prompt-status">
          {t("prompt.preparing", { name })}
        </p>
      )}
      {phase.kind === "failed" && (
        <p role="alert" className="settings-error">
          {phase.message}
        </p>
      )}
      {(phase.kind === "ready" || phase.kind === "installing") && (
        <>
          <p className="plugin-prompt-subtitle">
            {t("prompt.subtitle", {
              name,
              version: phase.offer.version,
              authors: phase.offer.authors.join(", "),
            })}
          </p>
          <PermissionList
            requested={requestedOf(phase.offer)}
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
            <fieldset className="plugin-prompt-visibility" disabled={installing}>
              <legend>{t("prompt.visibility")}</legend>
              {VISIBILITIES.map((choice) => (
                <label key={choice}>
                  <input
                    type="radio"
                    name={visibilityName}
                    value={choice}
                    checked={visibility === choice}
                    onChange={() => {
                      setVisibility(choice);
                    }}
                  />{" "}
                  {t(`prompt.visibility${capitalize(choice)}`)}
                </label>
              ))}
            </fieldset>
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
          {installing && (
            <p role="status" className="plugin-prompt-status">
              {phase.stage === null
                ? t("prompt.stage.downloading")
                : t(`prompt.stage.${phase.stage}`)}
            </p>
          )}
        </>
      )}
      <div className="settings-dialog__actions">
        <button type="button" className="settings-button" disabled={installing} onClick={close}>
          {phase.kind === "failed" ? t("prompt.close") : t("prompt.cancel")}
        </button>
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
