/**
 * The "Which app's sound?" dialog (FR-7.83, mockup 08): opens while the engine asks which app's
 * sound to share. The list follows the apps that play sound right now; "No sound" and Cancel both
 * answer with no app.
 */
import {
  useEffect,
  useEffectEvent,
  useId,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { getApi, toEngineError, type SoundApp } from "../../api";
import { NoSoundIcon } from "./icons";
import "../app/app.css";
import "./soundAppPicker.css";

/** How often the open list asks the engine for the apps that play sound now. */
const REFRESH_MS = 2000;
const NO_SOUND = "none";

export interface SoundAppPickerProps {
  serverId: number;
  /** The apps the engine offered with its question. */
  apps: SoundApp[];
}

/** A stable colour per app, so the rows stay recognisable while the list refreshes. */
function appHue(name: string): number {
  let hash = 0;
  for (const char of name) hash = (hash * 31 + char.charCodeAt(0)) % 360;
  return hash;
}

type Problem = "gone" | "failed";

export function SoundAppPicker({ serverId, apps: offered }: SoundAppPickerProps) {
  const { t } = useTranslation("screenshare");
  const titleId = useId();
  const ref = useRef<HTMLDialogElement>(null);
  const [apps, setApps] = useState(offered);
  const [selected, setSelected] = useState<string | null>(offered[0]?.id ?? null);
  const [problem, setProblem] = useState<Problem | null>(null);
  const [busy, setBusy] = useState(false);

  const answer = async (app: string | null) => {
    setBusy(true);
    try {
      await getApi().screenShareChooseSound(serverId, app);
    } catch (error) {
      if (toEngineError(error).code === "notFound") {
        setProblem("gone");
        setSelected(null);
        await refresh();
      } else {
        // invalidInput means the question is gone already; the dialog closes with the share state.
        console.error("Answering the share sound question failed", error);
        setProblem("failed");
      }
    } finally {
      setBusy(false);
    }
  };

  const refresh = async () => {
    try {
      const now = await getApi().screenShareSoundApps();
      setApps(now);
      setSelected((was) =>
        was !== null && was !== NO_SOUND && !now.some((a) => a.id === was) ? null : was,
      );
    } catch (error) {
      console.error("Refreshing the apps that play sound failed", error);
    }
  };

  // Effect events, so the dialog is not reopened when these handlers change.
  const cancel = useEffectEvent(() => void answer(null));
  const tick = useEffectEvent(() => void refresh());

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.showModal();
    const onEscape = (event: Event) => {
      event.preventDefault();
      cancel();
    };
    dialog.addEventListener("cancel", onEscape);
    const timer = window.setInterval(tick, REFRESH_MS);
    return () => {
      window.clearInterval(timer);
      dialog.removeEventListener("cancel", onEscape);
      if (dialog.open) dialog.close();
      if (opener?.isConnected) opener.focus();
    };
  }, []);

  // The "No sound" row always stays last, below the apps.
  const ids = [...apps.map((a) => a.id), NO_SOUND];
  const move = (event: KeyboardEvent<HTMLDivElement>) => {
    const at = selected === null ? -1 : ids.indexOf(selected);
    let next: number | null = null;
    if (event.key === "ArrowDown") next = Math.min(at + 1, ids.length - 1);
    else if (event.key === "ArrowUp") next = Math.max(at - 1, 0);
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = ids.length - 1;
    else if (event.key === "Enter" && selected !== null && !busy) {
      event.preventDefault();
      void answer(selected === NO_SOUND ? null : selected);
    }
    if (next === null) return;
    event.preventDefault();
    setSelected(ids[next] ?? null);
    setProblem(null);
  };

  const optionId = (id: string) => `${titleId}-${id}`;
  const row = (id: string, name: string, icon: ReactNode, extra = "") => (
    // Keys are handled by the listbox, which holds the focus (aria-activedescendant).
    // eslint-disable-next-line jsx-a11y/click-events-have-key-events
    <div
      key={id}
      id={optionId(id)}
      role="option"
      tabIndex={-1}
      aria-selected={selected === id}
      className={`sound-picker__row${extra}`}
      onClick={() => {
        setSelected(id);
        setProblem(null);
      }}
    >
      {icon}
      <span className="sound-picker__name">{name}</span>
    </div>
  );

  // Rendered into <body>, never inside a form (#445/#464).
  return createPortal(
    <dialog
      ref={ref}
      className="app-dialog sound-picker"
      aria-modal="true"
      aria-labelledby={titleId}
    >
      <h2 id={titleId} className="app-dialog__title">
        {t("soundPicker.title")}
      </h2>
      <div
        role="listbox"
        aria-labelledby={titleId}
        aria-activedescendant={selected === null ? undefined : optionId(selected)}
        tabIndex={0}
        className="sound-picker__list"
        onKeyDown={move}
      >
        {apps.map((app) =>
          row(
            app.id,
            app.name,
            <span
              className="sound-picker__icon"
              style={{ "--hue": appHue(app.name) } as CSSProperties}
              aria-hidden="true"
            />,
          ),
        )}
        {row(
          NO_SOUND,
          t("soundPicker.noSound"),
          <span className="sound-picker__icon sound-picker__icon--none" aria-hidden="true">
            <NoSoundIcon size={14} />
          </span>,
          " sound-picker__row--none",
        )}
      </div>
      {apps.length === 0 && (
        <p className="sound-picker__empty" role="status">
          {t("soundPicker.empty")}
        </p>
      )}
      {problem !== null && (
        <p className="app-dialog__error" role="alert">
          {t(`soundPicker.${problem}`)}
        </p>
      )}
      <div className="app-dialog__actions">
        <button type="button" className="button" disabled={busy} onClick={() => void answer(null)}>
          {t("soundPicker.cancel")}
        </button>
        <button
          type="button"
          className="button button--primary"
          disabled={busy || selected === null}
          onClick={() => void answer(selected === NO_SOUND ? null : selected)}
        >
          {t("soundPicker.share")}
        </button>
      </div>
    </dialog>,
    document.body,
  );
}
