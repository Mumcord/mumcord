import {
  useCallback,
  useEffect,
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
import { AccessibilitySection } from "./sections/AccessibilitySection";
import { AppearanceSection } from "./sections/AppearanceSection";
import { AudioSection } from "./sections/AudioSection";
import { ChatHistorySection } from "./sections/ChatHistorySection";
import { DiagnosticsSection } from "./sections/DiagnosticsSection";
import { IdentitySection } from "./sections/IdentitySection";
import { LanguageSection } from "./sections/LanguageSection";
import { OverlaySection } from "./sections/OverlaySection";
import { PacketInspectorSection } from "./sections/PacketInspectorSection";
import { PrivacySection } from "./sections/PrivacySection";
import { ServersSection } from "./sections/ServersSection";
import { SoundsSection } from "./sections/SoundsSection";
import { VoiceSection } from "./sections/VoiceSection";
import { SettingsNavigationContext, type SettingsNavigation } from "./settingsNavigation";
import { SettingsSearch } from "./SettingsSearch";
import type { SearchResult } from "./settingsIndex";
import { findGroup, findTarget, headingOf, type JumpTarget } from "./settingsJump";
import { SECTION_IDS, type SectionId } from "./sectionIds";
import { useIsMacos } from "../../api/useIsMacos";
import { useDevToolsUnlocked } from "../inspector/devTools";
import { useSettings, type UpdateSettings } from "./useSettings";
import "./settings.css";

/** How long a search jump waits for a control that loads late before landing on its group. */
const JUMP_WAIT_MS = 1500;
/** How long the control a search jumped to stays highlighted. */
const HIGHLIGHT_MS = 2500;

function flash(element: HTMLElement) {
  element.classList.add("settings-highlight");
  window.setTimeout(() => {
    element.classList.remove("settings-highlight");
  }, HIGHLIGHT_MS);
}

const isSearchShortcut = (event: globalThis.KeyboardEvent) =>
  (event.ctrlKey || event.metaKey) &&
  !event.altKey &&
  !event.shiftKey &&
  event.key.toLowerCase() === "f";

function renderSection(id: SectionId, settings: Settings, update: UpdateSettings): ReactNode {
  const props = { settings, update };
  switch (id) {
    case "identity":
      return <IdentitySection {...props} />;
    case "servers":
      return <ServersSection {...props} />;
    case "privacy":
      return <PrivacySection {...props} />;
    case "audio":
      return <AudioSection {...props} />;
    case "voice":
      return <VoiceSection {...props} />;
    case "chatHistory":
      return <ChatHistorySection {...props} />;
    case "appearance":
      return <AppearanceSection {...props} />;
    case "accessibility":
      return <AccessibilitySection {...props} />;
    case "language":
      return <LanguageSection {...props} />;
    case "sounds":
      return <SoundsSection {...props} />;
    case "overlay":
      return <OverlaySection />;
    case "diagnostics":
      return <DiagnosticsSection {...props} />;
    case "about":
      return <AboutSection {...props} />;
    case "packetInspector":
      return <PacketInspectorSection />;
  }
}

/**
 * Settings content of the settings overlay (FR-1.39): a search field and a vertical list of
 * sections on the left (one Tab stop, arrow keys move) and the selected section on the right.
 * `initialSection` picks the section shown first. Both columns scroll on their own. Every change is saved immediately.
 */
export function SettingsScreen({
  initialSection,
  settingsRevision,
}: {
  initialSection?: SectionId;
  /** Changes when something outside the settings wrote them, e.g. the setup wizard rerun (#149). */
  settingsRevision?: number;
}) {
  const { t } = useTranslation("settings");
  const baseId = useId();
  const { settings, loadFailed, saveFailed, update } = useSettings(settingsRevision);
  // macOS has no overlay at all (QA.58).
  const macos = useIsMacos();
  // The packet inspector stays hidden until the developer tools are unlocked (issue #60).
  const devTools = useDevToolsUnlocked();
  const hidden = useMemo<readonly SectionId[]>(
    () => [
      ...(macos ? (["overlay"] as const) : []),
      ...(devTools ? [] : (["packetInspector"] as const)),
    ],
    [macos, devTools],
  );
  const sections = useMemo(() => SECTION_IDS.filter((id) => !hidden.includes(id)), [hidden]);
  const [active, setActive] = useState<SectionId>(initialSection ?? "identity");
  // Hiding the developer tools from their own section goes back to About.
  if (active === "packetInspector" && !devTools) setActive("about");
  const tabs = useRef(new Map<SectionId, HTMLButtonElement>());
  const [hostElement, setHostElement] = useState<HTMLDivElement | null>(null);
  const { dialogContext, blocking } = useDialogHost(hostElement);
  const [compatibilityFocusPending, setCompatibilityFocusPending] = useState(false);
  const [query, setQuery] = useState("");
  const [jump, setJump] = useState<SearchResult | null>(null);
  // The pick that already landed, so a later re-run (e.g. a language change) does not jump again.
  const landedRef = useRef<SearchResult | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const navigation = useMemo<SettingsNavigation>(
    () => ({
      compatibilityFocusPending,
      compatibilityFocused: () => {
        setCompatibilityFocusPending(false);
      },
    }),
    [compatibilityFocusPending],
  );

  const select = (id: SectionId) => {
    setJump(null);
    setActive(id);
    tabs.current.get(id)?.focus();
  };

  // Ctrl+F inside the settings belongs to the settings search, not the channel search behind.
  useEffect(() => {
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (!isSearchShortcut(event)) return;
      event.preventDefault();
      searchRef.current?.focus();
      searchRef.current?.select();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
    };
  }, []);

  const pick = (result: SearchResult) => {
    setQuery("");
    setActive(result.kind === "section" ? result.section : result.entry.section);
    setJump(result);
  };

  // After a search pick: wait until the section (and possibly late-loading parts of it) shows the
  // control, then focus and highlight it.
  useEffect(() => {
    const panel = panelRef.current;
    if (!jump || !panel || landedRef.current === jump) return;
    if (jump.kind === "section") {
      landedRef.current = jump;
      tabs.current.get(jump.section)?.focus();
      return;
    }
    // The index holds translation keys; the screen shows the texts.
    const group = jump.entry.group === undefined ? undefined : t(jump.entry.group);
    const control = jump.entry.control === undefined ? undefined : t(jump.entry.control);
    const land = (target: JumpTarget) => {
      target.focus.focus();
      target.focus.scrollIntoView({ block: "center" });
      flash(target.highlight);
      landedRef.current = jump;
    };
    const attempt = () => {
      const target = findTarget(panel, group, control);
      if (target) land(target);
      return target !== null;
    };
    if (attempt()) return;
    const observer = new MutationObserver(() => {
      if (attempt()) observer.disconnect();
    });
    observer.observe(panel, { childList: true, subtree: true });
    const timer = window.setTimeout(() => {
      observer.disconnect();
      // The control is not offered here (e.g. a Wayland-only switch): land on its group instead.
      const scope = (group !== undefined ? findGroup(panel, group) : null) ?? panel;
      const heading = headingOf(scope);
      land({ focus: heading, highlight: scope === panel ? heading : scope });
    }, JUMP_WAIT_MS);
    return () => {
      observer.disconnect();
      window.clearTimeout(timer);
    };
  }, [jump, t]);

  const onKeyDown = (event: KeyboardEvent) => {
    const index = sections.indexOf(active);
    const last = sections.length - 1;
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
    const next = sections[target];
    if (next) select(next);
  };

  const tabId = (id: SectionId) => `${baseId}-tab-${id}`;
  const panelId = (id: SectionId) => `${baseId}-panel-${id}`;

  return (
    <DialogHostContext value={dialogContext}>
      <SettingsNavigationContext value={navigation}>
        <div className="settings" inert={blocking}>
          <div className="settings__sidebar">
            <SettingsSearch
              query={query}
              onQueryChange={setQuery}
              onPick={pick}
              inputRef={searchRef}
              hidden={hidden}
            />
            <div
              role="tablist"
              aria-orientation="vertical"
              aria-label={t("sectionList")}
              className="settings__nav"
              hidden={query.trim() !== ""}
            >
              {sections.map((id) => (
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
          </div>
          <div
            ref={panelRef}
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
      </SettingsNavigationContext>
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
