/** Simple interactive plugin components (FR-6.41): buttons, toggle, select, slider. */
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import type { PluginComponent } from "../../../api";
import { Select } from "../../../components/Select";
import { usePluginAsset } from "./assets";
import { useRender } from "./context";

type Of<T extends PluginComponent["type"]> = Extract<PluginComponent, { type: T }>;

const NONE = { type: "none" } as const;

export function PluginButton({ label, action, kind, disabled, expanded }: Of<"button">) {
  const { send } = useRender();
  const variant = kind === "primary" ? " button--primary" : kind === "danger" ? " pr-danger" : "";
  return (
    <button
      type="button"
      className={`button${variant}`}
      disabled={disabled}
      aria-expanded={expanded ?? undefined}
      onClick={() => {
        send(action, NONE);
      }}
    >
      {label}
    </button>
  );
}

export function PluginIconButton({ icon, label, action, disabled }: Of<"iconButton">) {
  const { plugin, send } = useRender();
  const url = usePluginAsset(plugin, icon);
  return (
    <button
      type="button"
      className="button pr-icon-button"
      aria-label={label}
      data-tooltip={label}
      disabled={disabled}
      onClick={() => {
        send(action, NONE);
      }}
    >
      {url ? (
        // Masked, not an <img>: the icon takes the text colour, so a dark plugin icon stays visible on any theme.
        <span
          className="pr-icon-mask"
          aria-hidden="true"
          style={{ maskImage: `url("${url}")`, WebkitMaskImage: `url("${url}")` }}
        />
      ) : (
        <span aria-hidden="true">•</span>
      )}
    </button>
  );
}

export function PluginToggle({ label, value, action, disabled }: Of<"toggle">) {
  const { send } = useRender();
  return (
    <label className="pr-toggle">
      <span>{label}</span>
      <input
        type="checkbox"
        role="switch"
        checked={value}
        disabled={disabled}
        onChange={(e) => {
          send(action, { type: "boolean", value: e.currentTarget.checked });
        }}
      />
    </label>
  );
}

export function PluginSelect(props: Of<"select">) {
  const { label, options, selected, action, refreshAction, disabled } = props;
  const { send } = useRender();
  const { t } = useTranslation("pluginSlots");
  const id = useId();
  return (
    <div className="pr-field">
      <label htmlFor={id}>{label}</label>
      <div className="pr-select">
        <Select
          id={id}
          value={selected ?? ""}
          options={options}
          disabled={disabled}
          onChange={(value) => {
            send(action, { type: "text", value });
          }}
        />
        {refreshAction !== null && (
          <button
            type="button"
            className="button pr-icon-button"
            aria-label={t("select.refresh", { label })}
            data-tooltip={t("select.refresh", { label })}
            disabled={disabled}
            onClick={() => {
              send(refreshAction, NONE);
            }}
          >
            <span aria-hidden="true">↻</span>
          </button>
        )}
      </div>
    </div>
  );
}

/** Sends the value once the user lets go (pointer up, key up, leaving), not on every step. */
export function PluginSlider(props: Of<"slider">) {
  const { label, action, disabled } = props;
  // f32 values arrive as null when they are not finite; the host refuses those anyway.
  const min = props.min ?? 0;
  const max = props.max ?? 1;
  const step = props.step ?? 1;
  const value = props.value ?? min;
  const { send } = useRender();
  const id = useId();
  const [local, setLocal] = useState<{ from: number; value: number }>({ from: value, value });
  // A new value from the plugin replaces what the user left behind.
  const current = local.from === value ? local.value : value;
  const commit = () => {
    if (current === value) return;
    send(action, { type: "number", value: current });
  };
  return (
    <div className="pr-field">
      <label htmlFor={id}>{label}</label>
      <input
        id={id}
        type="range"
        min={min}
        max={max}
        step={step}
        value={current}
        disabled={disabled}
        onChange={(e) => {
          setLocal({ from: value, value: e.currentTarget.valueAsNumber });
        }}
        onPointerUp={commit}
        onKeyUp={commit}
        onBlur={commit}
      />
      <output htmlFor={id}>{current}</output>
    </div>
  );
}
