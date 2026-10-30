import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import {
  getApi,
  type Identity,
  type PttAvailability,
  type PttMouseStatus,
  type Settings,
} from "../../api";
import { usePreferences } from "../../app/preferences";
import { useView } from "../../app/view";
import { IdentityStep } from "./IdentityStep";
import { MicStep } from "./MicStep";
import { PrivacyStep } from "./PrivacyStep";
import { UpdatesStep } from "./UpdatesStep";
import { TalkModeStep, type TalkChoice } from "./TalkModeStep";
import "./wizard.css";

interface Loaded {
  settings: Settings;
  identity: Identity | null;
  availability: PttAvailability | null;
  mouseStatus: PttMouseStatus | null;
  linkHandlerActive: boolean | null;
  ingameOverlaySupported: boolean;
  portable: boolean;
}

type Load = { state: "loading" } | { state: "failed" } | ({ state: "ready" } & Loaded);

/** Optional lookups must not block the wizard; their absence only hides a hint. */
function optional<T>(promise: Promise<T>, what: string): Promise<T | null> {
  return promise.catch((cause: unknown) => {
    console.error(`${what} failed`, cause);
    return null;
  });
}

async function loadWizard(): Promise<Loaded> {
  const api = getApi();
  const [settings, identity, availability, mouseStatus, linkHandlerActive, ingameOverlay, info] =
    await Promise.all([
      api.settingsGet(),
      api.identityGet(),
      optional(api.pttAvailability(), "ptt availability"),
      optional(api.pttMouseStatus(), "ptt mouse status"),
      optional(api.linkHandlerStatus(), "link handler status"),
      optional(api.ingameOverlayState(), "in-game overlay state"),
      optional(api.appInfo(), "app info"),
    ]);
  return {
    settings,
    identity,
    availability,
    mouseStatus,
    linkHandlerActive,
    ingameOverlaySupported: ingameOverlay?.supported ?? false,
    portable: info?.portable ?? false,
  };
}

/** First-start wizard: identity → microphone → privacy → update hint → talk mode and start options (FR-1.1–FR-1.6, FR-1.48d). */
export function Wizard() {
  const { t } = useTranslation("wizard");
  const { finishWizard } = useView();
  const { applySettings } = usePreferences();
  const [load, setLoad] = useState<Load>({ state: "loading" });
  const [step, setStep] = useState(1);
  const [updateCheck, setUpdateCheck] = useState(false);
  // Focus follows step changes only, not the initial render.
  const [navigated, setNavigated] = useState(false);

  const start = useCallback(() => {
    loadWizard()
      .then((loaded) => {
        setLoad({ state: "ready", ...loaded });
      })
      .catch((cause: unknown) => {
        console.error("loading the wizard failed", cause);
        setLoad({ state: "failed" });
      });
  }, []);

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

  const { settings } = load;

  function goTo(next: number) {
    setNavigated(true);
    setStep(next);
  }

  async function saveSettings(next: Settings): Promise<Settings> {
    const saved = await getApi().settingsUpdate(next);
    setLoad((current) => (current.state === "ready" ? { ...current, settings: saved } : current));
    return saved;
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
      updateCheck: updateCheck ? "yes" : "no",
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

  switch (step) {
    case 1:
      return (
        <IdentityStep
          identity={load.identity}
          focusOnMount={navigated}
          onDone={(identity) => {
            setLoad({ ...load, identity });
            goTo(2);
          }}
        />
      );
    case 2:
      return (
        <MicStep
          settings={settings}
          focusOnMount={navigated}
          onDevicesChange={async (devices) => {
            await saveSettings({ ...settings, ...devices });
          }}
          onBack={() => {
            goTo(1);
          }}
          onNext={() => {
            goTo(3);
          }}
        />
      );
    case 3:
      return (
        <PrivacyStep
          focusOnMount={navigated}
          onBack={() => {
            goTo(2);
          }}
          onNext={() => {
            goTo(4);
          }}
        />
      );
    case 4:
      return (
        <UpdatesStep
          initial={settings.updateCheck === "yes"}
          focusOnMount={navigated}
          onBack={() => {
            goTo(3);
          }}
          onNext={(check) => {
            // Saved with the final step; going Back keeps the choice in the form state.
            setUpdateCheck(check);
            goTo(5);
          }}
        />
      );
    default:
      return (
        <TalkModeStep
          settings={settings}
          availability={load.availability}
          mouseStatus={load.mouseStatus}
          linkHandlerActive={load.linkHandlerActive}
          ingameOverlaySupported={load.ingameOverlaySupported}
          portable={load.portable}
          focusOnMount={navigated}
          onBack={() => {
            goTo(4);
          }}
          onFinish={finish}
        />
      );
  }
}
