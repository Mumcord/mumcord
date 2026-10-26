import { useTranslation } from "react-i18next";
import { usePreferences } from "../../app/preferences";
import { ThemePicker } from "../../components/ThemePicker";
import { swatchOf } from "../../components/themeSwatch";
import { findTheme } from "../../theme/appTheme";
import { MUMCORD_PRESET, PRESET_THEMES } from "../../theme/themeModel";
import { useThemeChoice } from "../../theme/themeStore";

const APP_THEME = "";

interface ServerThemeSelectProps {
  label: string;
  /** The server's theme; `null` (or a theme that no longer exists) means the app theme. */
  value: string | null;
  onChange: (themeId: string | null) => void;
}

/**
 * Theme circles of one server (FR-2.17, FR-2.63): "Use app theme" (in the app theme's colours)
 * first, then every theme.
 */
export function ServerThemeSelect({ label, value, onChange }: ServerThemeSelectProps) {
  const { t } = useTranslation("servers");
  const { resolvedTheme } = usePreferences();
  const { appThemeId, customThemes } = useThemeChoice();
  const known = value !== null && findTheme(value, customThemes) !== null;
  const appTheme = findTheme(appThemeId, customThemes) ?? MUMCORD_PRESET;
  return (
    <ThemePicker
      label={label}
      presets={[
        { ...swatchOf(appTheme, resolvedTheme), id: APP_THEME, name: t("theme.useApp") },
        ...PRESET_THEMES.map((theme) => swatchOf(theme, resolvedTheme)),
      ]}
      customThemes={customThemes.map((theme) => swatchOf(theme, resolvedTheme))}
      selectedId={known ? value : APP_THEME}
      onSelect={(next) => {
        onChange(next === APP_THEME ? null : next);
      }}
    />
  );
}
