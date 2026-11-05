import type { CSSProperties } from "react";
import "./pluginTile.css";

/** A stable hue (0-359) per plugin id, so a plugin keeps its tile colour everywhere. */
function pluginHue(pluginId: string): number {
  let hash = 2166136261;
  for (const char of pluginId) {
    hash = Math.imul(hash ^ (char.codePointAt(0) ?? 0), 16777619) >>> 0;
  }
  return hash % 360;
}

/**
 * The icon of a plugin that ships no icon image: its initial on a gradient in the plugin's own
 * colour (surfaces mockup 06). Decorative, the name is always next to it. The caller's
 * `className` sets the size.
 */
export function PluginTile({
  id,
  name,
  className,
}: {
  id: string;
  name: string;
  className?: string;
}) {
  const initial = Array.from(name.trim())[0] ?? Array.from(id)[0] ?? "?";
  return (
    <span
      className={className ? `plugin-tile ${className}` : "plugin-tile"}
      style={{ "--plugin-hue": pluginHue(id) } as CSSProperties}
      aria-hidden="true"
    >
      {initial.toUpperCase()}
    </span>
  );
}
