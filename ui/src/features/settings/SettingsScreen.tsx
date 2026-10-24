import {
  useCallback,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import type { Settings } from "../../api";
import { DialogHostContext, type DialogHost } from "./components/dialogHost";
import { AboutSection } from "./sections/AboutSection";
import { AppearanceSection } from "./sections/AppearanceSection";
import { AudioSection } from "./sections/AudioSection";
import { ChatHistorySection } from "./sections/ChatHistorySection";
import { DiagnosticsSection } from "./sections/DiagnosticsSection";
import { IdentitySection } from "./sections/IdentitySection";
import { LanguageSection } from "./sections/LanguageSection";
import { OverlaySection } from "./sections/OverlaySection";
import { SoundsSection } from "./sections/SoundsSection";
import { VoiceSection } from "./sections/VoiceSection";
import { SECTION_IDS, type SectionId } from "./sectionIds";
import { useSettings, type UpdateSettings } from "./useSettings";
import "./settings.css";

function renderSection(id: SectionId, settings: Settings, update: UpdateSettings): ReactNode {
  const props = { settings, update };
  switch (id) {
    case "identity":
      return <IdentitySection {...props} />;
    case "audio":
      return <AudioSection {...props} />;
    case "voice":
      return <VoiceSection {...props} />;
    case "chatHistory":
      return <ChatHistorySection />;
    case "appearance":
      return <AppearanceSection {...props} />;
    case "language":
      return <LanguageSection {...props} />;
    case "sounds":
      return <SoundsSection {...props} />;
    case "overlay":
      return <OverlaySection />;
    case "diagnostics":
      return <DiagnosticsSection {...props} />;
    case "about":
      return <AboutSection />;
  }
}

/**
 * Settings content of the settings overlay (FR-1.39): a vertical list of sections on the left
 * (one Tab stop, arrow keys move) and the selected section on the right. Every change is saved
 * immediately.
 */
export function SettingsScreen() {
  const { t } = useTranslation("settings");
  const baseId = useId();
  const { settings, loadFailed, saveFailed, update } = useSettings();
  const [active, setActive] = useState<SectionId>("identity");
  const tabs = useRef(new Map<SectionId, HTMLButtonElement>());
  const [hostElement, setHostElement] = useState<HTMLDivElement | null>(null);
  const { dialogContext, blocking } = useDialogHost(hostElement);

  const select = (id: SectionId) => {
    setActive(id);
    tabs.current.get(id)?.focus();
  };

  const onKeyDown = (event: KeyboardEvent) => {
    const index = SECTION_IDS.indexOf(active);
    const last = SECTION_IDS.length - 1;
    const target =
      event.key === "ArrowDown"
        ? index === last
          ? 0
          : index + 1
        : event.key === "ArrowUp"
          ? index === 0
            ? last
            : index - 1
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? last
              : null;
    if (target === null) return;
    event.preventDefault();
    const next = SECTION_IDS[target];
    if (next) select(next);
  };

  const tabId = (id: SectionId) => `${baseId}-tab-${id}`;
  const panelId = (id: SectionId) => `${baseId}-panel-${id}`;

  return (
    <DialogHostContext value={dialogContext}>
      <div className="settings" inert={blocking}>
        <div
          role="tablist"
          aria-orientation="vertical"
          aria-label={t("sectionList")}
          className="settings__nav"
        >
          {SECTION_IDS.map((id) => (
            <button
              key={id}
              ref={(element) => {
                if (element) tabs.current.set(id, element);
                else tabs.current.delete(id);
              }}
              id={tabId(id)}
              type="button"
              role="tab"
              aria-selected={id === active}
              aria-controls={panelId(id)}
              tabIndex={id === active ? 0 : -1}
              className="settings__tab"
              onKeyDown={onKeyDown}
              onClick={() => {
                select(id);
              }}
            >
              {t(`sections.${id}`)}
            </button>
          ))}
        </div>
        <div
          role="tabpanel"
          id={panelId(active)}
          aria-labelledby={tabId(active)}
          className="settings__panel"
        >
          {saveFailed && (
            <p role="alert" className="settings-error">
              {t("saveFailed")}
            </p>
          )}
          {loadFailed ? (
            <p role="alert" className="settings-error">
              {t("loadFailed")}
            </p>
          ) : settings === null ? (
            <p className="settings-hint">{t("loading")}</p>
          ) : (
            renderSection(active, settings, update)
          )}
        </div>
      </div>
      <div ref={setHostElement} />
    </DialogHostContext>
  );
}

/**
 * Inner dialogs render into an element next to the settings content, which turns inert while
 * one is open; after the last one closes, focus goes back to the control that opened it.
 */
function useDialogHost(element: HTMLElement | null) {
  const [openCount, setOpenCount] = useState(0);
  const restore = useRef<HTMLElement | null>(null);

  const open = useCallback((opener: HTMLElement | null) => {
    setOpenCount((count) => {
      if (count === 0) restore.current = opener;
      return count + 1;
    });
    return () => {
      setOpenCount((count) => count - 1);
    };
  }, []);

  useLayoutEffect(() => {
    if (openCount > 0 || !restore.current) return;
    if (restore.current.isConnected) restore.current.focus();
    restore.current = null;
  }, [openCount]);

  const dialogContext = useMemo<DialogHost>(() => ({ element, open }), [element, open]);
  return { dialogContext, blocking: openCount > 0 };
}
