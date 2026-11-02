/**
 * Plugin colours are theme token names only (FR-6.41); this maps the plugin API's tokens
 * (`api::theme::TOKENS` in the Rust crate) to Mumcord's CSS variables, so plugin UI follows
 * the user's theme automatically.
 */
export const TOKEN_VARS = {
  text: "--color-text",
  "text-muted": "--color-text-muted",
  accent: "--color-accent",
  danger: "--color-danger",
  warning: "--color-warning",
  success: "--color-success",
  surface: "--color-bg-chat",
  "surface-raised": "--color-bg-elevated",
  border: "--color-border",
} as const;

export type ThemeToken = keyof typeof TOKEN_VARS;

/** A CSS colour for a token; unknown or missing tokens give `undefined` (the default colour). */
export function tokenColor(token: string | null): string | undefined {
  if (token === null || !Object.hasOwn(TOKEN_VARS, token)) return undefined;
  return `var(${TOKEN_VARS[token as ThemeToken]})`;
}
