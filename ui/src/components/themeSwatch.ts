import type { ThemeColors, ThemeRecord, ThemeVariant } from "../theme/themeModel";

/** One circle: a theme, or a stand-in like "Use app theme" shown in some theme's colours. */
export interface ThemeSwatch {
  id: string;
  name: string;
  colors: ThemeColors;
}

export function swatchOf(theme: ThemeRecord, variant: ThemeVariant): ThemeSwatch {
  return { id: theme.id, name: theme.name, colors: theme[variant] };
}

/** Where a circle's context menu opens; the circle gets the focus back when it closes. */
export interface SwatchMenuRequest {
  x: number;
  y: number;
  opener: HTMLElement;
}
