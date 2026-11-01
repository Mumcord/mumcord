import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { useTranslation } from "react-i18next";
import { matchingPreset } from "../wizard/presets";
import { ActivePresetCard } from "./ActivePresetCard";
import { changedSections } from "./changes";
import { DialogHostContext, type DialogHost } from "./components/dialogHost";
import { ChangedSettingsContext } from "./components/changedSettings";
import { SettingsIcon } from "./components/SettingsIcon";
import { SettingsNavigationContext, type SettingsNavigation } from "./settingsNavigation";
import { SettingsSearch } from "./SettingsSearch";
import type { SearchResult } from "./settingsIndex";
import { findGroup, findTarget, headingOf, type JumpTarget } from "./settingsJump";
import { SettingsPageCards } from "./SettingsPages";
import { SECTION_ICONS, SECTION_IDS, SET_APART_SECTIONS, type SectionId } from "./sectionIds";
import { useIsMacos } from "../../api/useIsMacos";
import { useDevToolsUnlocked } from "../inspector/devTools";
import { ResetSectionButton } from "./ResetSectionButton";
import { useSettings } from "./useSettings";
import { useSettingsChanges } from "./useSettingsChanges";
import "./settings.css";

/** Card title keys of cards that only show on some systems; the search hides their entries. */
const IN_GAME_OVERLAY_CARD = "overlay.inGame.title";
const DEV_TOOLS_CARD = "inspector.title";

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

/**
 * Settings content of the settings overlay (FR-1.39, #161): a search field and a vertical list of
 * groups on the left (one Tab stop, arrow keys move; Advanced & Developer and About set apart
 * below a divider) and the selected group's page on the right: header plus a grid of section
 * cards. `initialSection` picks the group shown first. Both columns scroll on their own. Every
 * change is saved immediately. Changed settings get a badge and their group a dot; the sidebar
 * footer shows the active preset and each page header offers "Reset section".
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
  const changes = useSettingsChanges(settings, update);
  const { current: currentDraft, changed } = changes;
  const changedLabels = useMemo(
    () => new Set(changed.map((setting) => t(setting.label))),
    [changed, t],
  );
  const dotSections = useMemo(() => changedSections(changed), [changed]);
  const presetMatch = useMemo(
    () => (currentDraft === null ? null : matchingPreset(currentDraft)),
    [currentDraft],
  );
  // macOS has no overlay at all (QA.58).
  const macos = useIsMacos();
  // The developer tools stay hidden until they are unlocked (issue #60).
  const devTools = useDevToolsUnlocked();
  const hiddenCards = useMemo(
    () => [...(macos ? [IN_GAME_OVERLAY_CARD] : []), ...(devTools ? [] : [DEV_TOOLS_CARD])],
    [macos, devTools],
  );
  const [active, setActive] = useState<SectionId>(initialSection ?? "general");
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
      jumpTo: (entry) => {
        setQuery("");
        setActive(entry.section);
        setJump({ kind: "setting", entry });
      },
      openCompatibilityMode: () => {
        setQuery("");
        setJump(null);
        setActive("voiceAudio");
        setCompatibilityFocusPending(true);
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
    const group = t(jump.entry.group);
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
      const scope = findGroup(panel, group) ?? panel;
      const heading = headingOf(scope);
      land({ focus: heading, highlight: scope === panel ? heading : scope });
    }, JUMP_WAIT_MS);
    return () => {
      observer.disconnect();
      window.clearTimeout(timer);
    };
  }, [jump, t]);

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
      <SettingsNavigationContext value={navigation}>
        <div className="settings" inert={blocking}>
          <div className="settings__sidebar">
            <SettingsSearch
              query={query}
              onQueryChange={setQuery}
              onPick={pick}
              inputRef={searchRef}
              hiddenCards={hiddenCards}
            />
            <div
              role="tablist"
              aria-orientation="vertical"
              aria-label={t("sectionList")}
              className="settings__nav"
              hidden={query.trim() !== ""}
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
                  aria-describedby={dotSections.has(id) ? `${baseId}-changed` : undefined}
                  tabIndex={id === active ? 0 : -1}
                  className={
                    SET_APART_SECTIONS[0] === id
                      ? "settings__tab settings__tab--apart"
                      : "settings__tab"
                  }
                  onKeyDown={onKeyDown}
                  onClick={() => {
                    select(id);
                  }}
                >
                  <SettingsIcon name={SECTION_ICONS[id]} />
                  {t(`sections.${id}`)}
                  {dotSections.has(id) && <span className="settings__tab-dot" aria-hidden="true" />}
                </button>
              ))}
            </div>
            {/* Describes the tabs with a dot; outside them, so a tab's text stays the group name. */}
            <span id={`${baseId}-changed`} hidden>
              {t("badges.groupChanged")}
            </span>
            <div className="settings__sidebar-footer">
              <ActivePresetCard changes={changes} />
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
            {changes.applyFailed && (
              <p role="alert" className="settings-error">
                {t("activePreset.applyFailed")}
              </p>
            )}
            <header className="settings-page__header">
              <div className="settings-page__heading">
                <p className="settings-page__breadcrumb" aria-hidden="true">
                  {t("breadcrumb", { section: t(`sections.${active}`) })}
                </p>
                <h2 className="settings-page__title">{t(`sections.${active}`)}</h2>
                <p className="settings-page__description">{t(`sectionDescriptions.${active}`)}</p>
              </div>
              <div className="settings-page__actions">
                <ResetSectionButton section={active} changes={changes} />
              </div>
            </header>
            {loadFailed ? (
              <p role="alert" className="settings-error">
                {t("loadFailed")}
              </p>
            ) : settings === null ? (
              <p className="settings-hint">{t("loading")}</p>
            ) : (
              <ChangedSettingsContext value={changedLabels}>
                <div className="settings-page__cards">
                  <SettingsPageCards
                    id={active}
                    settings={settings}
                    update={update}
                    macos={macos}
                    devTools={devTools}
                    presetMatch={presetMatch}
                  />
                </div>
              </ChangedSettingsContext>
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
