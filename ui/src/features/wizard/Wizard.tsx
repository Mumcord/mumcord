import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { getApi, type Identity, type PttAvailability, type Settings } from "../../api";
import { usePreferences } from "../../app/preferences";
import { useView } from "../../app/view";
import { IdentityStep } from "./IdentityStep";
import { MicStep } from "./MicStep";
import { TalkModeStep, type TalkChoice } from "./TalkModeStep";
import "./wizard.css";

interface Loaded {
  settings: Settings;
  identity: Identity | null;
  availability: PttAvailability | null;
  linkHandlerActive: boolean | null;
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
  const [settings, identity, availability, linkHandlerActive] = await Promise.all([
    api.settingsGet(),
    api.identityGet(),
    optional(api.pttAvailability(), "ptt availability"),
    optional(api.linkHandlerStatus(), "link handler status"),
  ]);
  return { settings, identity, availability, linkHandlerActive };
}

/** First-start wizard: identity → microphone → talk mode (FR-1.1–FR-1.6). */
export function Wizard() {
  const { t } = useTranslation("wizard");
  const { finishWizard } = useView();
  const { applySettings } = usePreferences();
  const [load, setLoad] = useState<Load>({ state: "loading" });
  const [step, setStep] = useState(1);
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
    const saved = await saveSettings({
      ...settings,
      talkMode: choice.talkMode,
      pttKey: choice.pttKey,
      wizardCompleted: true,
    });
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
    default:
      return (
        <TalkModeStep
          settings={settings}
          availability={load.availability}
          linkHandlerActive={load.linkHandlerActive}
          focusOnMount={navigated}
          onBack={() => {
            goTo(2);
          }}
          onFinish={finish}
        />
      );
  }
}
