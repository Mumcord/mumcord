import { useMemo } from "react";
import { decodeThemeCode, findThemeCode, type ThemeDecodeResult } from "../../theme/shareCodec";
import { useAppVersion } from "../settings/theme";

/** The shared theme a message carries, `null` when it has none or the code is unusable (FR-2.16). */
export function useSharedTheme(html: string): Extract<ThemeDecodeResult, { ok: true }> | null {
  const version = useAppVersion();
  return useMemo(() => {
    const code = findThemeCode(html);
    if (code === null || version === null) return null;
    const result = decodeThemeCode(code, version);
    return result.ok ? result : null;
  }, [html, version]);
}

const SHARE_PREFIX = /(?:🎨\s*Mumcord theme\s+(?:"|&quot;).*?(?:"|&quot;)\s*[–-]\s*)?$/u;

/**
 * The message HTML without the share line (prefix and code), so only the card shows. Other text
 * stays; line breaks left dangling by the removal are trimmed.
 */
export function withoutThemeShare(html: string, code: string): string {
  const at = html.indexOf(code);
  if (at < 0) return html;
  const before = html.slice(0, at).replace(SHARE_PREFIX, "");
  const after = html.slice(at + code.length);
  const edge = /^(?:\s|<br\s*\/?>)+|(?:\s|<br\s*\/?>)+$/gi;
  const joined =
    before.replace(edge, "") +
    (before.trim() && after.trim() ? "<br>" : "") +
    after.replace(edge, "");
  return joined;
}
