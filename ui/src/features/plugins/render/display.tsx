/** Display-only plugin components (FR-6.41): text, image, meter, chip, divider, progress. */
import { useTranslation } from "react-i18next";
import type { PluginComponent } from "../../../api";
import { usePluginAsset } from "./assets";
import { useRender } from "./context";
import { tokenColor } from "./tokens";

type Of<T extends PluginComponent["type"]> = Extract<PluginComponent, { type: T }>;

export function PluginText({ text, style, color }: Of<"text">) {
  const Tag = style === "heading" ? "h3" : style === "monospace" ? "code" : "span";
  return (
    <Tag className={`pr-text pr-text--${style}`} style={{ color: tokenColor(color) }}>
      {text}
    </Tag>
  );
}

/** An image asset; shows the alt text while it cannot be shown. */
export function PluginImage({ asset, alt, width, height }: Of<"image">) {
  const { plugin } = useRender();
  const url = usePluginAsset(plugin, asset);
  if (url === null) {
    return alt === "" ? null : <span className="pr-alt">{alt}</span>;
  }
  return (
    <img
      className="pr-image"
      src={url}
      alt={alt}
      width={width}
      height={height}
      draggable={false}
      aria-hidden={alt === "" ? true : undefined}
    />
  );
}

const percent = (value: number) => Math.round(Math.min(1, Math.max(0, value)) * 100);

export function PluginMeter({ label, value: raw, color }: Of<"meter">) {
  const value = raw ?? 0;
  return (
    <div className="pr-meter">
      <span className="pr-meter__label">{label}</span>
      <meter
        className="pr-meter__bar"
        aria-label={label}
        min={0}
        max={1}
        value={value}
        style={{ ["--pr-meter-color" as string]: tokenColor(color) ?? "var(--color-accent)" }}
      >
        {percent(value)}%
      </meter>
    </div>
  );
}

export function PluginChip({ text, color, tone }: Of<"chip">) {
  if (tone !== null) return <span className={`pr-chip pr-tone--${tone}`}>{text}</span>;
  const c = tokenColor(color);
  return (
    <span className="pr-chip" style={c === undefined ? undefined : { color: c, borderColor: c }}>
      {text}
    </span>
  );
}

export function PluginDivider() {
  return <hr className="pr-divider" />;
}

export function PluginProgress({ label, value }: Of<"progress">) {
  const { t } = useTranslation("pluginSlots");
  return (
    <div className="pr-progress">
      <span className="pr-progress__label">{label}</span>
      <progress
        className="pr-progress__bar"
        aria-label={label}
        aria-valuetext={value === null ? t("progress.working") : `${String(percent(value))}%`}
        max={1}
        value={value ?? undefined}
      />
    </div>
  );
}
