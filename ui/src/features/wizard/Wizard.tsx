import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import {
  getApi,
  type Identity,
  type MumbleImportFound,
  type PttAvailability,
  type PttMouseStatus,
  type Settings,
  type SetupPreset,
} from "../../api";
import { usePreferences } from "../../app/preferences";
import { useView } from "../../app/view";
import { updateModeOf, type UpdateMode } from "../updates/updateMode";
import { AppearanceStep } from "./AppearanceStep";
import { ConnectionsStep } from "./ConnectionsStep";
import { FeaturesStep } from "./FeaturesStep";
import { IdentityStep } from "./IdentityStep";
import { ImportStep } from "./ImportStep";
import { matchingPreset, RECOMMENDED_PRESET } from "./presets";
import { PrivateMessagesStep } from "./PrivateMessagesStep";
import { SetupStyleStep } from "./SetupStyleStep";
import { StartupStep } from "./StartupStep";
import { StepNavContext, type StepNav } from "./stepNav";
import { SummaryStep } from "./SummaryStep";
import { VoiceStep } from "./VoiceStep";
import { WelcomeStep } from "./WelcomeStep";
import { WizardCancelDialog } from "./WizardCancelDialog";
import {
  applyStyle,
  changedSettings,
  presetValuesChanged,
  recordedPreset,
  type WizardDraft,
} from "./wizardDraft";
import {
  back,
  canCancel,
  canGoBack,
  canSkip,
  createWizardFlow,
  CUSTOM_STEPS,
  editFromSummary,
  next,
  primaryButton,
  progress,
  railEntries,
  selectStyle,
  skip,
  type WizardFlowState,
  type WizardStepId,
} from "./wizardFlow";
import { WizardRail } from "./WizardRail";
import "../settings/settings.css";
import "./wizard.css";

interface Loaded {
  /** What is stored; the reference for "what did the wizard change". */
  settings: Settings;
  identity: Identity | null;
  availability: PttAvailability | null;
  mouseStatus: PttMouseStatus | null;
  linkHandlerActive: boolean | null;
  /** In-game overlay on, or `null` where it is unsupported. */
  ingameOverlay: boolean | null;
  dndFollowSystem: boolean;
  portable: boolean;
  /** The official Mumble client's data, when it is installed (first start only, issue #66). */
  mumble: MumbleImportFound | null;
}

type Load = { state: "loading" } | { state: "failed" } | ({ state: "ready" } & Loaded);

/** What the wizard edits, and what it started from. */
interface Editing {
  draft: WizardDraft;
  /** The draft as loaded, before any preset: overlay and DND are saved only when they differ. */
  initial: WizardDraft;
  initialUpdateMode: UpdateMode;
  flow: WizardFlowState;
  /** The user picked a setup style card (or Custom through a summary Edit). */
  styleChosen: boolean;
}

/** Optional lookups must not block the wizard; their absence only hides a choice. */
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
    overlay,
    dnd,
    info,
    mumble,
  ] = await Promise.all([
    api.settingsGet(),
    api.identityGet(),
    optional(api.pttAvailability(), "ptt availability"),
    optional(api.pttMouseStatus(), "ptt mouse status"),
    optional(api.linkHandlerStatus(), "link handler status"),
    optional(api.ingameOverlayState(), "in-game overlay state"),
    optional(api.dndState(), "do not disturb state"),
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
    ingameOverlay: overlay?.supported === true ? overlay.mode === "on" : null,
    dndFollowSystem: dnd?.followSystem ?? false,
    portable: info?.portable ?? false,
    mumble,
  };
}

/**
 * First start: Balanced is preselected and applied. Rerun: the saved values stay as they are,
 * and the card that matches them is preselected (Custom when none does).
 */
function startEditing(loaded: Loaded, rerun: boolean): Editing {
  const initial: WizardDraft = {
    settings: loaded.settings,
    ingameOverlay: loaded.ingameOverlay,
    dndFollowSystem: loaded.dndFollowSystem,
    // Portable builds (#181) leave system changes such as the link handler unticked.
    registerLinkHandler: loaded.linkHandlerActive !== true && !loaded.portable,
  };
  const initialUpdateMode = updateModeOf(loaded.settings);
  const flow = createWizardFlow({
    mode: rerun ? "rerun" : "firstStart",
    mumbleFound: loaded.mumble !== null,
  });
  if (!rerun) {
    return {
      draft: applyStyle(initial, RECOMMENDED_PRESET, initialUpdateMode),
      initial,
      initialUpdateMode,
      flow,
      styleChosen: false,
    };
  }
  const saved: SetupPreset =
    loaded.settings.setupPreset === "custom" ? "custom" : (matchingPreset(initial) ?? "custom");
  return {
    draft: initial,
    initial,
    initialUpdateMode,
    flow: selectStyle(flow, saved),
    styleChosen: false,
  };
}

/** The settings the window previews while the wizard runs: language and look only. */
function previewOf(base: Settings, draft: Settings): Settings {
  return {
    ...base,
    language: draft.language,
    theme: draft.theme,
    appThemeId: draft.appThemeId,
    visualEffects: draft.visualEffects,
  };
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
 * Setup wizard (issue #159, FR-3.115–FR-3.118): a step rail and one step at a time, driven by
 * `wizardFlow`. Every choice lives in a draft; nothing is written before Finish, and Finish
 * writes only what differs from the stored settings. In rerun mode (#149) the identity step is
 * left out, every value starts at the saved one and Cancel keeps everything.
 */
export function Wizard({ mode = "firstStart", onClose, onManageIdentities }: WizardProps) {
  const { t } = useTranslation("wizard");
  const { finishWizard } = useView();
  const { applySettings } = usePreferences();
  const rerun = mode === "rerun";
  const [load, setLoad] = useState<Load>({ state: "loading" });
  const [editing, setEditing] = useState<Editing | null>(null);
  // Focus follows step changes only, not the initial render; the rerun starts with its heading.
  const [navigated, setNavigated] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [cancelAsked, setCancelAsked] = useState(false);
  // The identity before an import from Mumble, restored when the user undoes it (#66).
  const firstIdentity = useRef<Identity | null>(null);
  // The stored settings, and the preview the window shows (as JSON; `null`: none), for cleanup.
  const stored = useRef<Settings | null>(null);
  const shownPreview = useRef<string | null>(null);
  const finished = useRef(false);

  const start = useCallback(() => {
    loadWizard(rerun)
      .then((loaded) => {
        firstIdentity.current = loaded.identity;
        stored.current = loaded.settings;
        setLoad({ state: "ready", ...loaded });
        setEditing(startEditing(loaded, rerun));
      })
      .catch((cause: unknown) => {
        console.error("loading the wizard failed", cause);
        setLoad({ state: "failed" });
      });
  }, [rerun]);

  useEffect(start, [start]);

  // Language and look apply to the window at once (the live preview) without being saved.
  const draftSettings = editing?.draft.settings ?? null;
  useEffect(() => {
    const base = stored.current;
    if (draftSettings === null || base === null) return;
    const preview = previewOf(base, draftSettings);
    const json = JSON.stringify(preview);
    const differs = json !== JSON.stringify(base);
    // Only when the previewed values change, not on every other edit of the draft.
    if (differs ? json !== shownPreview.current : shownPreview.current !== null) {
      applySettings(preview);
    }
    shownPreview.current = differs ? json : null;
  }, [applySettings, draftSettings]);

  // Leaving without Finish (rerun Cancel) takes the preview back.
  useEffect(
    () => () => {
      if (shownPreview.current !== null && !finished.current && stored.current !== null) {
        applySettings(stored.current);
      }
    },
    [applySettings],
  );

  if (load.state === "loading" || (load.state === "ready" && editing === null)) {
    return <p role="status">{t("common:loading")}</p>;
  }
  if (load.state === "failed" || editing === null) {
    return (
      <div className="wizard-failed">
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

  const loaded = load;
  const { draft, initial, flow } = editing;
  const settings = draft.settings;

  function go(state: WizardFlowState) {
    setNavigated(true);
    setError(null);
    setEditing((current) => (current === null ? current : { ...current, flow: state }));
  }

  function changeDraft(change: (current: WizardDraft) => WizardDraft) {
    setEditing((current) =>
      current === null ? current : { ...current, draft: change(current.draft) },
    );
  }

  function update(patch: Partial<Settings>) {
    changeDraft((current) => ({ ...current, settings: { ...current.settings, ...patch } }));
  }

  function chooseStyle(style: SetupPreset) {
    setEditing((current) => {
      if (current === null) return current;
      return {
        ...current,
        flow: selectStyle(current.flow, style),
        styleChosen: true,
        draft:
          style === "custom"
            ? current.draft
            : applyStyle(current.draft, style, current.initialUpdateMode),
      };
    });
  }

  /** Summary → Edit. A Custom-only card while a preset is chosen switches to Custom first. */
  function edit(step: WizardStepId) {
    const switching = CUSTOM_STEPS.includes(step) && flow.style !== "custom";
    if (switching)
      setEditing((current) => (current === null ? current : { ...current, styleChosen: true }));
    go(editFromSummary(switching ? selectStyle(flow, "custom") : flow, step));
  }

  /** The Mumble import (#66) saves at once; its values join the draft and the stored base. */
  async function saveImport(next: Settings): Promise<Settings> {
    const before = loaded.settings;
    const saved = await getApi().settingsUpdate(next);
    stored.current = saved;
    setLoad((current) => (current.state === "ready" ? { ...current, settings: saved } : current));
    update(changedSettings(before, saved));
    return saved;
  }

  function hasChanges(): boolean {
    return (
      Object.keys(changedSettings(loaded.settings, settings)).length > 0 ||
      draft.ingameOverlay !== initial.ingameOverlay ||
      draft.dndFollowSystem !== initial.dndFollowSystem
    );
  }

  function requestCancel() {
    if (hasChanges()) setCancelAsked(true);
    else onClose?.();
  }

  /** Runs an optional system change; a failure is reported but does not stop Finish. */
  async function attempt(what: string, action: () => Promise<unknown>, failure: string) {
    try {
      await action();
    } catch (cause: unknown) {
      console.error(`${what} failed`, cause);
      announce(failure, "polite");
    }
  }

  async function finish() {
    setBusy(true);
    setError(null);
    const api = getApi();
    try {
      const record = !rerun || editing?.styleChosen === true || presetValuesChanged(initial, draft);
      const target: Settings = record
        ? { ...settings, setupPreset: recordedPreset(flow.style, draft) }
        : settings;
      // Only what the wizard changed; anything changed elsewhere meanwhile stays as it is.
      const { autostart, ...patch } = changedSettings(loaded.settings, target);
      let saved = await api.settingsGet();
      if (!rerun || Object.keys(patch).length > 0) {
        saved = await api.settingsUpdate({
          ...saved,
          ...patch,
          ...(rerun ? {} : { wizardCompleted: true }),
        });
      }
      // Saved on its own: if the OS refuses the login entry, the wizard still finishes.
      if (autostart !== undefined) {
        await attempt(
          "setting up start at login",
          async () => {
            saved = await api.settingsUpdate({ ...saved, autostart });
          },
          t("startup.autostartFailed"),
        );
      }
      if (draft.registerLinkHandler && loaded.linkHandlerActive !== true) {
        await attempt(
          "registering the link handler",
          () => api.linkHandlerRegister(),
          t("startup.linkHandlerFailed"),
        );
      }
      if (draft.ingameOverlay !== null && draft.ingameOverlay !== initial.ingameOverlay) {
        const on = draft.ingameOverlay;
        await attempt(
          "changing the in-game overlay",
          () => api.ingameOverlaySetMode(on ? "on" : "off"),
          t("errors.generic"),
        );
      }
      if (draft.dndFollowSystem !== initial.dndFollowSystem) {
        const follow = draft.dndFollowSystem;
        await attempt(
          "changing follow system Do Not Disturb",
          () => api.dndSetFollowSystem(follow),
          t("errors.generic"),
        );
      }
      finished.current = true;
      applySettings(saved);
      if (rerun) onClose?.();
      else finishWizard();
    } catch (cause: unknown) {
      console.error("finishing the setup failed", cause);
      setBusy(false);
      setError(t("errors.saveFailed"));
    }
  }

  const entries = railEntries(flow);
  const nav: StepNav = {
    number: entries.findIndex((entry) => entry.state === "current") + 1,
    total: entries.length,
    name: t(`steps.${flow.step}`),
    focusOnMount: rerun || navigated,
    primary: flow.step === "welcome" ? "getStarted" : primaryButton(flow),
    onBack: canGoBack(flow)
      ? () => {
          go(back(flow));
        }
      : undefined,
    onSkip: canSkip(flow)
      ? () => {
          go(skip(flow));
        }
      : undefined,
    onCancel: canCancel(flow) ? requestCancel : undefined,
  };

  const toNext = () => {
    go(next(flow));
  };

  function stepBody(): ReactNode {
    switch (flow.step) {
      case "welcome":
        return <WelcomeStep settings={settings} update={update} onNext={toNext} />;
      case "import":
        return loaded.mumble === null ? null : (
          <ImportStep
            step={nav.number}
            total={nav.total}
            found={loaded.mumble}
            settings={loaded.settings}
            saveSettings={saveImport}
            onIdentityChange={(imported) => {
              setLoad((current) =>
                current.state === "ready"
                  ? { ...current, identity: imported ?? firstIdentity.current }
                  : current,
              );
            }}
            onNext={toNext}
          />
        );
      case "identity":
        return (
          <IdentityStep
            identity={loaded.identity}
            onDone={(done) => {
              setLoad((current) =>
                current.state === "ready" ? { ...current, identity: done } : current,
              );
              toNext();
            }}
          />
        );
      case "voice":
        return (
          <VoiceStep
            settings={settings}
            update={update}
            availability={loaded.availability}
            mouseStatus={loaded.mouseStatus}
            onNext={toNext}
          />
        );
      case "setupStyle":
        return <SetupStyleStep style={flow.style} onSelect={chooseStyle} onNext={toNext} />;
      case "privateMessages":
        return <PrivateMessagesStep settings={settings} update={update} onNext={toNext} />;
      case "connections":
        return <ConnectionsStep settings={settings} update={update} onNext={toNext} />;
      case "features":
        return (
          <FeaturesStep
            settings={settings}
            update={update}
            ingameOverlay={draft.ingameOverlay}
            dndFollowSystem={draft.dndFollowSystem}
            onIngameOverlayChange={(on) => {
              changeDraft((current) => ({ ...current, ingameOverlay: on }));
            }}
            onDndFollowSystemChange={(on) => {
              changeDraft((current) => ({ ...current, dndFollowSystem: on }));
            }}
            onNext={toNext}
          />
        );
      case "appearance":
        return <AppearanceStep settings={settings} update={update} onNext={toNext} />;
      case "startup":
        return (
          <StartupStep
            settings={settings}
            update={update}
            linkHandlerActive={loaded.linkHandlerActive}
            registerLinkHandler={draft.registerLinkHandler}
            onRegisterLinkHandlerChange={(on) => {
              changeDraft((current) => ({ ...current, registerLinkHandler: on }));
            }}
            onNext={toNext}
          />
        );
      case "summary":
        return (
          <SummaryStep
            draft={draft}
            identity={rerun ? null : loaded.identity}
            name={loaded.identity?.name ?? null}
            firstStart={!rerun}
            busy={busy}
            error={error}
            onEdit={edit}
            onFinish={() => void finish()}
          />
        );
    }
  }

  return (
    <div className="wizard-layout">
      <WizardRail
        entries={entries}
        progress={progress(flow)}
        note={
          rerun ? (
            <p className="wizard-small">
              {t("rerun.note")}{" "}
              <button type="button" className="wizard-link" onClick={onManageIdentities}>
                {t("rerun.manageIdentities")}
              </button>
            </p>
          ) : undefined
        }
      />
      <div className="wizard-main">
        <StepNavContext.Provider value={nav}>
          {/* A new key per step remounts it, so its heading takes the focus. */}
          <div key={flow.step} className="wizard-main__step">
            {stepBody()}
          </div>
        </StepNavContext.Provider>
      </div>
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
    </div>
  );
}
