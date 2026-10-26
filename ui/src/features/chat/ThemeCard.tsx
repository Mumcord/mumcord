/**
 * Card under a message that carries a theme share code (FR-2.16): name, a mini mock-up in the
 * theme's colours and the apply actions. Official clients just see the plain string.
 */
import { useEffect, useRef, useState, type CSSProperties, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { usePreferences } from "../../app/preferences";
import { useShownServerId } from "../channels";
import { saveServerTheme, useServers } from "../servers";
import { displayName } from "../servers/format";
import { useIsImported, useThemeActions } from "../settings/theme";
import type { SharedTheme } from "../../theme/shareCodec";
import { useSharedTheme } from "./sharedTheme";

/** The version matching the Dark / Light / System setting, like the theme would show. */
function MockUp({ theme }: { theme: SharedTheme }) {
  const { resolvedTheme } = usePreferences();
  const { background, surface, text, accent, talking } = theme[resolvedTheme];
  const style = {
    "--mock-background": background,
    "--mock-surface": surface,
    "--mock-text": text,
    "--mock-accent": accent,
    "--mock-talking": talking,
  } as CSSProperties;
  return (
    <div className="theme-card__mock" style={style} aria-hidden="true">
      <div className="theme-card__mock-side">
        <span className="theme-card__mock-talking" />
        <span className="theme-card__mock-line" />
        <span className="theme-card__mock-line" />
      </div>
      <div className="theme-card__mock-main">
        <span className="theme-card__mock-line theme-card__mock-line--long" />
        <span className="theme-card__mock-bubble" />
        <span className="theme-card__mock-line" />
      </div>
    </div>
  );
}

export function ThemeCard({ html, tabIndex }: { html: string; tabIndex: number }) {
  const shared = useSharedTheme(html);
  if (shared === null) return null;
  return (
    <Card
      theme={shared.theme}
      createdWith={shared.createdWith}
      newer={shared.madeWithNewer}
      tabIndex={tabIndex}
    />
  );
}

interface CardProps {
  theme: SharedTheme;
  createdWith: string;
  newer: boolean;
  tabIndex: number;
}

function Card({ theme, createdWith, newer, tabIndex }: CardProps) {
  const { t } = useTranslation("chat");
  const { applyAppTheme, importTheme } = useThemeActions();
  const imported = useIsImported(theme);
  const { servers } = useServers();
  const shownId = useShownServerId();
  const [open, setOpen] = useState(false);
  const [failed, setFailed] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  const toggleRef = useRef<HTMLButtonElement>(null);

  // The shown server comes first (QB.2).
  const ordered = [
    ...servers.filter((s) => s.id === shownId),
    ...servers.filter((s) => s.id !== shownId),
  ];

  useEffect(() => {
    if (open) menuRef.current?.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  }, [open]);

  const run = (action: () => Promise<unknown>) => {
    setFailed(false);
    action().catch((error: unknown) => {
      console.error("Applying the shared theme failed", error);
      setFailed(true);
    });
  };

  const close = (returnFocus: boolean) => {
    setOpen(false);
    if (returnFocus) toggleRef.current?.focus();
  };

  const onMenuKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const items = [...(menuRef.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])];
    const index = items.findIndex((item) => item === document.activeElement);
    const focusAt = (next: number) => items[(next + items.length) % items.length]?.focus();
    switch (event.key) {
      case "ArrowDown":
        focusAt(index + 1);
        break;
      case "ArrowUp":
        focusAt(index - 1);
        break;
      case "Home":
        focusAt(0);
        break;
      case "End":
        focusAt(items.length - 1);
        break;
      case "Escape":
      case "Tab":
        close(true);
        break;
      default:
        return;
    }
    event.preventDefault();
    event.stopPropagation();
  };

  return (
    <section className="theme-card" aria-label={t("themeCard.label", { name: theme.name })}>
      <MockUp theme={theme} />
      <div className="theme-card__body">
        <p className="theme-card__name">{theme.name}</p>
        {newer && (
          <p className="theme-card__note">{t("themeCard.newer", { version: createdWith })}</p>
        )}
        {failed && (
          <p className="theme-card__note" role="alert">
            {t("themeCard.failed")}
          </p>
        )}
        <div className="theme-card__actions">
          {imported && <span className="theme-card__imported">{t("themeCard.imported")}</span>}
          <div className="theme-card__split">
            <button
              type="button"
              className="theme-card__apply"
              tabIndex={tabIndex}
              onClick={() => {
                run(() => applyAppTheme(theme));
              }}
            >
              {t("themeCard.apply")}
            </button>
            <button
              ref={toggleRef}
              type="button"
              className="theme-card__arrow"
              tabIndex={tabIndex}
              aria-haspopup="menu"
              aria-expanded={open}
              aria-label={t("themeCard.more")}
              disabled={servers.length === 0}
              onClick={() => {
                setOpen((value) => !value);
              }}
            >
              <span aria-hidden="true">▾</span>
            </button>
            {open && (
              <div
                ref={menuRef}
                className="theme-card__menu"
                role="menu"
                tabIndex={-1}
                aria-label={t("themeCard.menuLabel")}
                onKeyDown={onMenuKey}
                onBlur={(event) => {
                  if (
                    !(
                      event.relatedTarget instanceof Node &&
                      event.currentTarget.contains(event.relatedTarget)
                    ) &&
                    event.relatedTarget !== toggleRef.current
                  )
                    setOpen(false);
                }}
              >
                {ordered.map((server) => (
                  <button
                    key={server.id}
                    type="button"
                    role="menuitem"
                    tabIndex={-1}
                    className="theme-card__item"
                    onClick={() => {
                      close(true);
                      run(async () => {
                        await saveServerTheme(server.id, await importTheme(theme));
                      });
                    }}
                  >
                    {t("themeCard.applyToServer", { server: displayName(server) })}
                  </button>
                ))}
              </div>
            )}
          </div>
        </div>
      </div>
    </section>
  );
}
