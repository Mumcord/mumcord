import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import {
  getApi,
  type Identity,
  type MumbleImportFound,
  type P2pMode,
  type PttAvailability,
  type PttMouseStatus,
  type Settings,
} from "../../api";
import { usePreferences } from "../../app/preferences";
import { useView } from "../../app/view";
import { IdentityStep } from "./IdentityStep";
import { ImportStep } from "./ImportStep";
import { MicStep } from "./MicStep";
import { PrivacyStep } from "./PrivacyStep";
import { UpdatesStep } from "./UpdatesStep";
import { updateModeOf, updateModePatch, type UpdateMode } from "../updates/updateMode";
import { TalkModeStep, type TalkChoice } from "./TalkModeStep";
import { WizardSteps } from "./WizardSteps";
import { WizardCancelDialog } from "./WizardCancelDialog";
import "./wizard.css";

/** First start: [import from Mumble →] identity → microphone → privacy → update hint → talk mode and start options. */
const FIRST_START_STEPS = 5;
/** Rerun from Settings (#149): the same steps, without the identity step. */
const RERUN_STEPS = 4;

interface Loaded {
  settings: Settings;
  identity: Identity | null;
  availability: PttAvailability | null;
  mouseStatus: PttMouseStatus | null;
  linkHandlerActive: boolean | null;
  ingameOverlaySupported: boolean;
  ingameOverlayOn: boolean;
  portable: boolean;
  /** The official Mumble client's data, when it is installed (first start only, issue #66). */
  mumble: MumbleImportFound | null;
}

type Load = { state: "loading" } | { state: "failed" } | ({ state: "ready" } & Loaded);

/** Optional lookups must not block the wizard; their absence only hides a hint. */
function optional<T>(promise: Promise<T>, what: string): Promise<T | null> {
  return promise.catch((cause: unknown) => {
    console.error(`${what} failed`, cause);
    return null;
  });
}

async function loadWizard(rerun: boolean): Promise<Loaded> {
  const api = getApi();
  const [
    settings,
    identity,
    availability,
    mouseStatus,
    linkHandlerActive,
    ingameOverlay,
    info,
    mumble,
  ] = await Promise.all([
    api.settingsGet(),
    api.identityGet(),
    optional(api.pttAvailability(), "ptt availability"),
    optional(api.pttMouseStatus(), "ptt mouse status"),
    optional(api.linkHandlerStatus(), "link handler status"),
    optional(api.ingameOverlayState(), "in-game overlay state"),
    optional(api.appInfo(), "app info"),
    rerun
      ? Promise.resolve(null)
      : optional(api.mumbleImportScan(), "looking for the official Mumble client"),
  ]);
  return {
    settings,
    identity,
    availability,
    mouseStatus,
    linkHandlerActive,
    ingameOverlaySupported: ingameOverlay?.supported ?? false,
    ingameOverlayOn: ingameOverlay?.mode === "on",
    portable: info?.portable ?? false,
    mumble,
  };
}

/** Whether the rerun's start options differ from the saved ones, so Cancel would drop them. */
function startOptionsChanged(choice: TalkChoice, saved: Settings, overlayOn: boolean): boolean {
  return (
    choice.talkMode !== saved.talkMode ||
    choice.pttKey !== saved.pttKey ||
    choice.linkPreviews !== saved.linkPreviews ||
    choice.historySharing !== saved.historySharing ||
    choice.autostart !== saved.autostart ||
    // The link handler is an opt-in action, not a saved value: leaving it ticked is no change.
    (choice.ingameOverlay !== null && choice.ingameOverlay !== overlayOn)
  );
}

interface WizardProps {
  /** First start (default) or the rerun from Settings → About (#149). */
  mode?: "firstStart" | "rerun";
  /** Rerun only: closes the wizard without saving, called by Cancel and after Finish. */
  onClose?: (() => void) | undefined;
  /** Rerun only: the note's link to the identity manager (#148). */
  onManageIdentities?: (() => void) | undefined;
}

/**
 * Setup wizard: identity → microphone → privacy → update hint → talk mode and start options
 * (FR-1.1–FR-1.6, FR-1.48d). In rerun mode (#149, FR-3.118) the identity step is left out, every
 * value starts at the saved one and nothing is written before Finish.
 */
export function Wizard({ mode = "firstStart", onClose, onManageIdentities }: WizardProps) {
  const { t } = useTranslation("wizard");
  const { finishWizard } = useView();
  const { applySettings } = usePreferences();
  const rerun = mode === "rerun";
  const [load, setLoad] = useState<Load>({ state: "loading" });
  const [step, setStep] = useState(1);
  // Saved settings plus the rerun's not yet saved devices (#149).
  const [draft, setDraft] = useState<Settings | null>(null);
  const [updateMode, setUpdateMode] = useState<UpdateMode>("off");
  // The privacy step's peer-to-peer mode (spec 04, FR-4.1), saved with the final step.
  const [p2pMode, setP2pMode] = useState<P2pMode>("directRelay");
  // The talk mode step's current choices, for the "would Cancel drop something?" question.
  const [startOptions, setStartOptions] = useState<TalkChoice | null>(null);
  const [cancelAsked, setCancelAsked] = useState(false);
  // Focus follows step changes only, not the initial render; the rerun starts with its heading.
  const [navigated, setNavigated] = useState(false);
  // The identity before an import from Mumble, restored when the user undoes it (#66).
  const firstIdentity = useRef<Identity | null>(null);

  const start = useCallback(() => {
    loadWizard(rerun)
      .then((loaded) => {
        firstIdentity.current = loaded.identity;
        setLoad({ state: "ready", ...loaded });
        // The optional import from Mumble (#66) comes before the identity step.
        if (loaded.mumble !== null) setStep(0);
        setDraft(loaded.settings);
        setUpdateMode(updateModeOf(loaded.settings));
        setP2pMode(loaded.settings.p2pMode);
      })
      .catch((cause: unknown) => {
        console.error("loading the wizard failed", cause);
        setLoad({ state: "failed" });
      });
  }, [rerun]);

  useEffect(start, [start]);

  if (load.state === "loading") {
    return <p role="status">{t("common:loading")}</p>;
  }
  if (load.state === "failed") {
    return (
      <div className="wizard-step">
        <p role="alert" className="wizard-error">
          {t("loadFailed")}
        </p>
        <button
          type="button"
          className="button button--primary"
          onClick={() => {
            setLoad({ state: "loading" });
            start();
          }}
        >
          {t("common:retry")}
        </button>
      </div>
    );
  }

  const saved = load.settings;
  // Everything the steps need, so the helpers below do not depend on the union above.
  const {
    availability,
    mouseStatus,
    linkHandlerActive,
    ingameOverlaySupported,
    ingameOverlayOn,
    portable,
    identity,
    mumble,
  } = load;
  // Rerun steps show the draft; the first start shows what is saved (the microphone step saves
  // its devices at once, so the test runs on them).
  const settings = rerun ? (draft ?? saved) : saved;
  // The import step (#66) moves every later step's number up by one.
  const shift = mumble === null ? 0 : 1;
  const total = (rerun ? RERUN_STEPS : FIRST_START_STEPS) + shift;
  const focusOnMount = rerun || navigated;

  function goTo(next: number) {
    setNavigated(true);
    setStep(next);
  }

  async function saveSettings(next: Settings): Promise<Settings> {
    const stored = await getApi().settingsUpdate(next);
    setLoad((current) => (current.state === "ready" ? { ...current, settings: stored } : current));
    return stored;
  }

  async function finish(choice: TalkChoice) {
    if (choice.registerLinkHandler) {
      try {
        await getApi().linkHandlerRegister();
      } catch (cause: unknown) {
        // Not worth blocking the first start; Settings offers it again.
        console.error("registering the link handler failed", cause);
        announce(t("talk.linkHandlerFailed"), "polite");
      }
    }
    let saved = await saveSettings({
      ...settings,
      talkMode: choice.talkMode,
      pttKey: choice.pttKey,
      linkPreviews: choice.linkPreviews,
      historySharing: choice.historySharing,
      ...updateModePatch(updateMode),
      p2pMode,
      wizardCompleted: true,
    });
    if (choice.autostart !== saved.autostart) {
      // Saved on its own: if the OS refuses the login entry, the wizard still finishes.
      try {
        saved = await saveSettings({ ...saved, autostart: choice.autostart });
      } catch (cause: unknown) {
        console.error("setting up start at login failed", cause);
        announce(t("talk.autostartFailed"), "polite");
      }
    }
    if (choice.ingameOverlay) {
      try {
        await getApi().ingameOverlaySetMode("on");
      } catch (cause: unknown) {
        // Not worth blocking the first start; Settings offers it again.
        console.error("turning on the in-game overlay failed", cause);
      }
    }
    applySettings(saved);
    finishWizard();
  }

  /** Rerun (#149): writes only the values the user changed, and only now. */
  async function finishRerun(choice: TalkChoice) {
    // Read fresh: anything changed elsewhere while the wizard was open stays as it is.
    const current = await getApi().settingsGet();
    const next: Settings = {
      ...current,
      inputDevice: settings.inputDevice,
      outputDevice: settings.outputDevice,
      ...updateModePatch(updateMode),
      p2pMode,
      talkMode: choice.talkMode,
      pttKey: choice.pttKey,
      linkPreviews: choice.linkPreviews,
      historySharing: choice.historySharing,
      autostart: choice.autostart,
    };
    const changed =
      next.inputDevice !== current.inputDevice ||
      next.outputDevice !== current.outputDevice ||
      next.updateCheck !== current.updateCheck ||
      next.updateAutoDownload !== current.updateAutoDownload ||
      next.p2pMode !== current.p2pMode ||
      next.talkMode !== current.talkMode ||
      next.pttKey !== current.pttKey ||
      next.linkPreviews !== current.linkPreviews ||
      next.historySharing !== current.historySharing ||
      next.autostart !== current.autostart;
    const stored = changed ? await getApi().settingsUpdate(next) : current;
    if (choice.registerLinkHandler) {
      try {
        await getApi().linkHandlerRegister();
      } catch (cause: unknown) {
        console.error("registering the link handler failed", cause);
        announce(t("talk.linkHandlerFailed"), "polite");
      }
    }
    if (choice.ingameOverlay !== null && choice.ingameOverlay !== ingameOverlayOn) {
      try {
        await getApi().ingameOverlaySetMode(choice.ingameOverlay ? "on" : "off");
      } catch (cause: unknown) {
        console.error("changing the in-game overlay failed", cause);
        announce(t("errors.generic"), "polite");
      }
    }
    applySettings(stored);
    onClose?.();
  }

  function requestCancel() {
    const pending = rerun
      ? settings.inputDevice !== saved.inputDevice ||
        settings.outputDevice !== saved.outputDevice ||
        updateMode !== updateModeOf(saved) ||
        p2pMode !== saved.p2pMode ||
        (startOptions !== null && startOptionsChanged(startOptions, saved, ingameOverlayOn))
      : false;
    if (pending) {
      setCancelAsked(true);
      return;
    }
    onClose?.();
  }

  const cancel = rerun ? requestCancel : undefined;

  function micStep(number: number, back: (() => void) | undefined) {
    return (
      <MicStep
        step={number + shift}
        total={total}
        settings={settings}
        stagedDevices={rerun}
        focusOnMount={focusOnMount}
        onDevicesChange={async (devices) => {
          if (rerun) {
            setDraft({ ...settings, ...devices });
            return;
          }
          await saveSettings({ ...settings, ...devices });
        }}
        onBack={back}
        onNext={() => {
          goTo(number + 1);
        }}
        onCancel={cancel}
      />
    );
  }

  function privacyStep(number: number) {
    return (
      <PrivacyStep
        step={number + shift}
        total={total}
        p2pMode={p2pMode}
        onP2pModeChange={setP2pMode}
        focusOnMount={focusOnMount}
        onBack={() => {
          goTo(number - 1);
        }}
        onNext={() => {
          goTo(number + 1);
        }}
        onCancel={cancel}
      />
    );
  }

  function updatesStep(number: number) {
    return (
      <UpdatesStep
        step={number + shift}
        total={total}
        initial={updateMode}
        focusOnMount={focusOnMount}
        onBack={() => {
          goTo(number - 1);
        }}
        // Saved with the final step; going Back keeps the choice in the form state.
        onChange={setUpdateMode}
        onNext={() => {
          goTo(number + 1);
        }}
        onCancel={cancel}
      />
    );
  }

  function talkStep(number: number) {
    return (
      <TalkModeStep
        step={number + shift}
        total={total}
        settings={settings}
        availability={availability}
        mouseStatus={mouseStatus}
        linkHandlerActive={linkHandlerActive}
        ingameOverlaySupported={ingameOverlaySupported}
        initialIngameOverlay={rerun && ingameOverlayOn}
        portable={portable}
        focusOnMount={focusOnMount}
        onBack={() => {
          goTo(number - 1);
        }}
        onCancel={cancel}
        onChange={rerun ? setStartOptions : undefined}
        onFinish={rerun ? finishRerun : finish}
      />
    );
  }

  function stepBody(): ReactNode {
    if (rerun) {
      switch (step) {
        case 1:
          return micStep(1, undefined);
        case 2:
          return privacyStep(2);
        case 3:
          return updatesStep(3);
        default:
          return talkStep(4);
      }
    }
    switch (step) {
      case 0:
        return mumble === null ? null : (
          <ImportStep
            step={1}
            total={total}
            found={mumble}
            settings={settings}
            saveSettings={saveSettings}
            onIdentityChange={(imported) => {
              setLoad((current) =>
                current.state === "ready"
                  ? { ...current, identity: imported ?? firstIdentity.current }
                  : current,
              );
            }}
            onNext={() => {
              goTo(1);
            }}
          />
        );
      case 1:
        return (
          <IdentityStep
            step={1 + shift}
            total={total}
            identity={identity}
            focusOnMount={navigated}
            onDone={(done) => {
              setLoad((current) =>
                current.state === "ready" ? { ...current, identity: done } : current,
              );
              goTo(2);
            }}
          />
        );
      case 2:
        return micStep(2, () => {
          goTo(1);
        });
      case 3:
        return privacyStep(3);
      case 4:
        return updatesStep(4);
      default:
        return talkStep(5);
    }
  }

  const body = stepBody();
  if (!rerun) return body;

  return (
    <>
      <WizardSteps current={step} total={RERUN_STEPS} />
      <p className="wizard-note wizard-rerun__note">
        {t("rerun.note")}{" "}
        <button type="button" className="wizard-link" onClick={onManageIdentities}>
          {t("rerun.manageIdentities")}
        </button>
      </p>
      {body}
      <p className="wizard-muted wizard-small wizard-rerun__footer">{t("rerun.footer")}</p>
      {cancelAsked && (
        <WizardCancelDialog
          onKeepEditing={() => {
            setCancelAsked(false);
          }}
          onDiscard={() => {
            setCancelAsked(false);
            onClose?.();
          }}
        />
      )}
    </>
  );
}
