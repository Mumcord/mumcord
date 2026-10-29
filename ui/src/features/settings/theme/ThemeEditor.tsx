import { createPortal } from "react-dom";
import { useEffect, useId, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type Theme } from "../../../api";
import { usePreferences } from "../../../app/preferences";
import { Select } from "../../../components/Select";
import { ThemePicker } from "../../../components/ThemePicker";
import { swatchOf, type SwatchMenuRequest } from "../../../components/themeSwatch";
import {
  findImported,
  findTheme,
  fromStoredTheme,
  newCustomThemeId,
  selectedAppThemeId,
  toStoredTheme,
} from "../../../theme/appTheme";
import { validateTheme, type ThemeIssue } from "../../../theme/derive";
import { cleanThemeName, formatShareMessage, type SharedTheme } from "../../../theme/shareCodec";
import {
  isPresetId,
  MUMCORD_PRESET,
  MUMCORD_PRESET_ID,
  PRESET_THEMES,
  THEME_COLOR_KEYS,
  THEME_VARIANTS,
  type ThemeColorKey,
  type ThemeColors,
  type ThemeRecord,
  type ThemeVariant,
} from "../../../theme/themeModel";
import { setThemePreview } from "../../../theme/themeStore";
import { editVersion, type ThemeVersions } from "../../../theme/versions";
import { ContextMenu, type MenuItem } from "../../../components/ContextMenu";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { ModalDialog } from "../components/ModalDialog";
import type { SettingsSectionProps } from "../sections/types";
import { ColorField } from "./ColorField";
import { ImportThemeDialog } from "./ImportThemeDialog";
import { RenameThemeDialog } from "./RenameThemeDialog";
import { useAppVersion } from "./useAppVersion";
import "./themeEditor.css";

type Status = { kind: "success" | "error"; text: string } | null;

const MODES: readonly Theme[] = ["system", "dark", "light"];

/** The columns of the colour table, light first like the spec's "Light | Dark". */
const COLUMNS: readonly ThemeVariant[] = ["light", "dark"];

/** What the editor is open on: a new theme started from `theme`, or an own theme to change. */
interface Editing {
  theme: ThemeRecord;
  isNew: boolean;
  /** Tells drafts apart, so opening the editor again starts a fresh one. */
  key: number;
}

/** A circle's context menu. */
interface OpenMenu {
  theme: ThemeRecord;
  request: SwatchMenuRequest;
}

/**
 * App theme in Settings → Appearance (FR-2.14 – FR-2.16, FR-2.63): the Dark / Light / System
 * setting, the theme circles with "Your themes" and "+", Import, and the editor with both
 * versions' key colours side by side. While it is open the window shows the app theme, or the
 * draft while the editor is open, even when the shown server has its own theme.
 */
export function ThemeEditor({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const id = useId();
  const version = useAppVersion();
  const { resolvedTheme } = usePreferences();
  const [importing, setImporting] = useState(false);
  const [status, setStatus] = useState<Status>(null);
  const [editing, setEditing] = useState<Editing | null>(null);
  const [menu, setMenu] = useState<OpenMenu | null>(null);
  const [renaming, setRenaming] = useState<ThemeRecord | null>(null);
  const [deleting, setDeleting] = useState<ThemeRecord | null>(null);
  const customThemes = useMemo(
    () => settings.customThemes.map(fromStoredTheme),
    [settings.customThemes],
  );
  const selectedId = selectedAppThemeId(settings.appThemeId, customThemes);
  const selected = findTheme(selectedId, customThemes) ?? MUMCORD_PRESET;

  useEffect(
    () => () => {
      setThemePreview(null);
    },
    [],
  );

  // The open draft sets its own preview.
  useEffect(() => {
    if (editing === null) setThemePreview({ kind: "app", variant: resolvedTheme });
  }, [editing, resolvedTheme]);

  const openEditor = (theme: ThemeRecord, isNew: boolean) => {
    setStatus(null);
    setEditing({ theme, isNew, key: (editing?.key ?? 0) + 1 });
  };

  const choose = (themeId: string) => {
    setStatus(null);
    update({ appThemeId: themeId });
  };

  const saveTheme = (theme: ThemeRecord, select: boolean) => {
    const stored = toStoredTheme(theme);
    const known = settings.customThemes.some((custom) => custom.id === theme.id);
    update({
      customThemes: known
        ? settings.customThemes.map((custom) => (custom.id === theme.id ? stored : custom))
        : [...settings.customThemes, stored],
      ...(select ? { appThemeId: theme.id } : {}),
    });
  };

  const onImport = (shared: SharedTheme, newerVersion: string | null) => {
    setImporting(false);
    const existing = findImported(shared, customThemes);
    const theme = existing ?? { id: newCustomThemeId(), ...shared };
    saveTheme(theme, true);
    const done = existing
      ? t("themeEditor.alreadyImported", { name: theme.name })
      : t("themeEditor.imported", { name: theme.name });
    const newer = newerVersion
      ? ` ${t("themeEditor.madeWithNewer", { version: newerVersion })}`
      : "";
    setStatus({ kind: "success", text: done + newer });
  };

  const onDelete = (themeId: string) => {
    setStatus(null);
    if (editing?.theme.id === themeId) setEditing(null);
    update({
      customThemes: settings.customThemes.filter((custom) => custom.id !== themeId),
      ...(settings.appThemeId === themeId ? { appThemeId: MUMCORD_PRESET_ID } : {}),
    });
  };

  const share = async (theme: ThemeRecord) => {
    if (version === null) return;
    try {
      await getApi().copyText(formatShareMessage(theme, version));
      setStatus({ kind: "success", text: t("themeEditor.shared") });
    } catch (error) {
      console.error("Copying the theme failed", error);
      setStatus({ kind: "error", text: t("themeEditor.shareFailed") });
    }
  };

  const menuItems = (theme: ThemeRecord): MenuItem[] => {
    const newFromThis = {
      label: t("themeEditor.newFromThis"),
      onSelect: () => {
        openEditor(theme, true);
      },
    };
    const shareItem = {
      label: t("themeEditor.share"),
      onSelect: () => {
        void share(theme);
      },
    };
    // Presets cannot be changed; "New theme from this" is their way to a personal copy.
    if (isPresetId(theme.id)) return [newFromThis, shareItem];
    return [
      {
        label: t("themeEditor.edit"),
        onSelect: () => {
          openEditor(theme, false);
        },
      },
      newFromThis,
      shareItem,
      {
        label: t("themeEditor.rename"),
        onSelect: () => {
          setRenaming(theme);
        },
      },
      {
        label: t("themeEditor.delete"),
        onSelect: () => {
          setDeleting(theme);
        },
      },
    ];
  };

  return (
    <>
      <div className="settings-control-row">
        <label htmlFor={`${id}-mode`}>{t("appearance.modeLegend")}</label>
        <Select
          id={`${id}-mode`}
          value={settings.theme}
          options={MODES.map((mode) => ({ value: mode, label: t(`appearance.${mode}`) }))}
          onChange={(value) => {
            const mode = MODES.find((candidate) => candidate === value);
            if (mode) update({ theme: mode });
          }}
        />
      </div>
      <ThemePicker
        label={t("appearance.themeLegend")}
        presets={PRESET_THEMES.map((theme) => swatchOf(theme, resolvedTheme))}
        customThemes={customThemes.map((theme) => swatchOf(theme, resolvedTheme))}
        selectedId={selectedId}
        onSelect={choose}
        onAdd={() => {
          openEditor(selected, true);
        }}
        onMenu={(themeId, request) => {
          const theme = findTheme(themeId, customThemes);
          if (theme) setMenu({ theme, request });
        }}
      />
      <div className="settings-actions">
        <button
          type="button"
          className="settings-button"
          disabled={version === null}
          onClick={() => {
            setStatus(null);
            setImporting(true);
          }}
        >
          {t("themeEditor.import")}
        </button>
      </div>
      {editing && (
        <ThemeDraft
          key={editing.key}
          theme={editing.theme}
          isNew={editing.isNew}
          initialVariant={resolvedTheme}
          onSave={(theme) => {
            saveTheme(theme, editing.isNew);
            setEditing(null);
            setStatus({ kind: "success", text: t("themeEditor.saved") });
          }}
          onCancel={() => {
            setEditing(null);
          }}
          onChange={() => {
            setStatus(null);
          }}
        />
      )}
      <p role="status" className={status?.kind === "error" ? "settings-error" : "settings-success"}>
        {status?.text}
      </p>
      {menu && (
        <ContextMenu
          menu={menu.request}
          label={t("themeEditor.menuLabel", { name: menu.theme.name })}
          items={menuItems(menu.theme)}
          onClose={() => {
            setMenu(null);
          }}
        />
      )}
      {renaming && (
        <RenameThemeDialog
          name={renaming.name}
          onRename={(name) => {
            setRenaming(null);
            setStatus(null);
            saveTheme({ ...renaming, name }, false);
          }}
          onCancel={() => {
            setRenaming(null);
          }}
        />
      )}
      {deleting && (
        <ConfirmDialog
          title={t("themeEditor.deleteTitle", { name: deleting.name })}
          message={t("themeEditor.deleteMessage")}
          confirmLabel={t("themeEditor.deleteConfirm")}
          onConfirm={() => {
            setDeleting(null);
            onDelete(deleting.id);
          }}
          onCancel={() => {
            setDeleting(null);
          }}
        />
      )}
      {importing && version !== null && (
        <ImportThemeDialog
          version={version}
          onImport={onImport}
          onClose={() => {
            setImporting(false);
          }}
        />
      )}
    </>
  );
}

interface ThemeDraftProps {
  theme: ThemeRecord;
  /** Saving adds a new own theme and applies it; otherwise it overwrites `theme`. */
  isNew: boolean;
  /** The version previewed when the draft opens: the one the window shows. */
  initialVariant: ThemeVariant;
  onSave: (theme: ThemeRecord) => void;
  onCancel: () => void;
  onChange: () => void;
}

function sameColors(a: ThemeColors, b: ThemeColors): boolean {
  return THEME_COLOR_KEYS.every((key) => a[key] === b[key]);
}

function versionsOf(theme: ThemeRecord): ThemeVersions {
  return { dark: theme.dark, light: theme.light, generated: theme.generated };
}

function ThemeDraft({ theme, isNew, initialVariant, onSave, onCancel, onChange }: ThemeDraftProps) {
  const { t } = useTranslation("settings");
  const id = useId();
  const initialName = isNew ? t("themeEditor.copyName", { name: theme.name }) : theme.name;
  // Preview hides the dialog and shows the whole app in the draft's colours (FR-2.63).
  const [previewing, setPreviewing] = useState(false);
  const [versions, setVersions] = useState(() => versionsOf(theme));
  const [name, setName] = useState(initialName);
  const dirty =
    THEME_VARIANTS.some((v) => !sameColors(versions[v], theme[v])) || name !== initialName;
  const validations = useMemo(
    () => ({ dark: validateTheme(versions.dark), light: validateTheme(versions.light) }),
    [versions],
  );
  const bothValid = validations.dark.ok && validations.light.ok;
  const cleanName = cleanThemeName(name);

  useEffect(() => {
    setThemePreview(
      previewing
        ? { kind: "draft", theme: { id: "draft", name, ...versions }, variant: initialVariant }
        : { kind: "app", variant: initialVariant },
    );
  }, [name, versions, previewing, initialVariant]);

  useEffect(() => {
    if (!previewing) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      setPreviewing(false);
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [previewing]);

  const setColor = (variant: ThemeVariant, key: ThemeColorKey, color: string) => {
    onChange();
    setVersions((current) => editVersion(current, variant, { ...current[variant], [key]: color }));
  };

  const issueOf = (variant: ThemeVariant, key: ThemeColorKey): ThemeIssue | undefined =>
    validations[variant].issues.find((issue) => issue.key === key);
  const issueId = (variant: ThemeVariant, key: ThemeColorKey) => `${id}-${key}-${variant}-issue`;

  if (previewing) {
    return createPortal(
      <div
        className="theme-preview-bar"
        role="region"
        aria-label={t("themeEditor.previewing", { name })}
      >
        <span>{t("themeEditor.previewing", { name })}</span>
        <button
          type="button"
          className="settings-button settings-button--primary"
          // eslint-disable-next-line jsx-a11y/no-autofocus
          autoFocus
          onClick={() => {
            setPreviewing(false);
          }}
        >
          {t("themeEditor.backToEditor")}
        </button>
      </div>,
      document.body,
    );
  }

  return (
    <ModalDialog
      title={isNew ? t("themeEditor.newTitle") : t("themeEditor.editTitle", { name: theme.name })}
      onClose={onCancel}
      className="theme-editor"
    >
      <div className="settings-control-row">
        <label htmlFor={`${id}-name`}>{t("themeEditor.name")}</label>
        <input
          id={`${id}-name`}
          className="settings-input theme-editor__name"
          value={name}
          aria-invalid={cleanName === ""}
          onChange={(event) => {
            onChange();
            setName(event.target.value);
          }}
        />
      </div>
      <table className="theme-table">
        <caption className="theme-editor__legend">{t("themeEditor.colorsLegend")}</caption>
        <thead>
          <tr>
            <th scope="col">{t("themeEditor.colorColumn")}</th>
            {COLUMNS.map((variant) => (
              <th key={variant} scope="col">
                {t(`appearance.${variant}`)}
                {versions.generated === variant && (
                  <span className="theme-table__auto">{t("themeEditor.auto")}</span>
                )}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {THEME_COLOR_KEYS.map((key) => {
            const label = t(`themeEditor.colors.${key}`);
            return (
              <tr key={key}>
                <th scope="row">
                  {label}
                  {COLUMNS.map((variant) => {
                    const issue = issueOf(variant, key);
                    return (
                      issue && (
                        <p
                          key={variant}
                          id={issueId(variant, key)}
                          className="settings-warning theme-color__note"
                        >
                          {t("themeEditor.contrastLowIn", {
                            version: t(`appearance.${variant}`),
                            ratio: issue.ratio.toFixed(1),
                            min: String(issue.min),
                          })}
                        </p>
                      )
                    );
                  })}
                </th>
                {COLUMNS.map((variant) => (
                  <td key={variant}>
                    <ColorField
                      label={t("themeEditor.cellLabel", {
                        color: label,
                        version: t(`appearance.${variant}`),
                      })}
                      value={versions[variant][key]}
                      issueId={issueOf(variant, key) && issueId(variant, key)}
                      onChange={(color) => {
                        setColor(variant, key, color);
                      }}
                    />
                  </td>
                ))}
              </tr>
            );
          })}
        </tbody>
      </table>
      {versions.generated !== null && (
        <p className="settings-hint">
          {t(
            versions.generated === "light"
              ? "themeEditor.generatedFromDark"
              : "themeEditor.generatedFromLight",
          )}
        </p>
      )}
      {!bothValid && (
        <p className="settings-error" role="alert">
          {t("themeEditor.unreadable")}
        </p>
      )}
      <div className="settings-actions">
        <button
          type="button"
          className="settings-button settings-button--primary"
          disabled={!bothValid || cleanName === ""}
          onClick={() => {
            onSave({ id: isNew ? newCustomThemeId() : theme.id, name: cleanName, ...versions });
          }}
        >
          {isNew ? t("themeEditor.saveAs") : t("themeEditor.save")}
        </button>
        <button
          type="button"
          className="settings-button"
          disabled={!dirty}
          onClick={() => {
            onChange();
            setVersions(versionsOf(theme));
            setName(initialName);
          }}
        >
          {t("themeEditor.reset")}
        </button>
        <button
          type="button"
          className="settings-button"
          onClick={() => {
            setPreviewing(true);
          }}
        >
          {t("themeEditor.preview")}
        </button>
        <button type="button" className="settings-button" onClick={onCancel}>
          {t("common:cancel")}
        </button>
      </div>
    </ModalDialog>
  );
}
