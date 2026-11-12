import { useState, type KeyboardEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useIsMacos } from "../../api/useIsMacos";
import { ComposerIcon } from "../../components/ComposerIcon";
import { EDITOR_ICONS } from "./richEditorIcons";
import type { CaretState, Format } from "./richEditorModel";

export type Tool = Exclude<Format, "link">;
type ToolKey = Tool | "link" | "image" | "color" | "source";

interface Shortcut {
  key: string;
  shift?: boolean;
}

const SHORTCUT: Partial<Record<ToolKey, Shortcut>> = {
  bold: { key: "B" },
  italic: { key: "I" },
  underline: { key: "U" },
  strike: { key: "X", shift: true },
  link: { key: "K" },
  bullets: { key: "8", shift: true },
  numbers: { key: "7", shift: true },
};

function shortcutHint({ key, shift }: Shortcut, macos: boolean): string {
  return macos ? `${shift ? "⇧" : ""}⌘${key}` : `Ctrl+${shift ? "Shift+" : ""}${key}`;
}

function ariaShortcut({ key, shift }: Shortcut, macos: boolean): string {
  return `${macos ? "Meta" : "Control"}+${shift ? "Shift+" : ""}${key}`;
}

interface ToolButtonProps {
  tool: ToolKey;
  label: string;
  /** The button roving focus currently rests on (the only one reached with Tab). */
  current: boolean;
  disabled: boolean;
  pressed?: boolean;
  /** Set for buttons that open a popover: whether it is open. */
  expanded?: boolean;
  active?: boolean;
  wide?: boolean;
  onFocus: (tool: ToolKey) => void;
  onClick: (button: HTMLButtonElement) => void;
  children: ReactNode;
}

function ToolButton(props: ToolButtonProps) {
  const macos = useIsMacos();
  const { tool, label, expanded } = props;
  const shortcut = SHORTCUT[tool];
  const popup = expanded !== undefined;
  const classes = ["rte__tool"];
  if (props.wide) classes.push("rte__tool--wide");
  if (props.active) classes.push("rte__tool--active");
  return (
    <button
      type="button"
      className={classes.join(" ")}
      aria-label={label}
      aria-pressed={props.pressed}
      aria-haspopup={popup ? "dialog" : undefined}
      aria-expanded={expanded}
      aria-keyshortcuts={shortcut ? ariaShortcut(shortcut, macos) : undefined}
      data-tooltip={label}
      data-tooltip-shortcut={shortcut ? shortcutHint(shortcut, macos) : undefined}
      data-tooltip-placement="bottom"
      data-tool={tool}
      data-popup-tool={popup || undefined}
      tabIndex={props.current ? 0 : -1}
      disabled={props.disabled}
      onMouseDown={(e) => {
        // Keeps the selection in the text while a button is pressed.
        e.preventDefault();
      }}
      onFocus={() => {
        props.onFocus(tool);
      }}
      onClick={(e) => {
        props.onClick(e.currentTarget);
      }}
    >
      {props.children}
    </button>
  );
}

interface RichEditorToolbarProps {
  caret: CaretState;
  popup: "color" | "link" | null;
  inSource: boolean;
  disabled: boolean;
  onFormat: (tool: Tool) => void;
  onColor: (button: HTMLButtonElement) => void;
  onLink: (button: HTMLButtonElement) => void;
  onImage: () => void;
  onSource: () => void;
}

/** The editor's toolbar: grouped icon buttons, active formatting at the caret, Source toggle. */
export function RichEditorToolbar({
  caret,
  popup,
  inSource,
  disabled,
  onFormat,
  onColor,
  onLink,
  onImage,
  onSource,
}: RichEditorToolbarProps) {
  const { t } = useTranslation("userpanel");
  const [focusTool, setFocusTool] = useState<ToolKey>("bold");
  const formatDisabled = disabled || inSource;
  const rovingTool = inSource ? "source" : focusTool;

  // Arrow keys move between the buttons; Tab enters and leaves the toolbar in one step.
  const keyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const target = event.target;
    if (!(target instanceof HTMLButtonElement) || target.dataset.tool === undefined) return;
    const buttons = Array.from(
      event.currentTarget.querySelectorAll<HTMLButtonElement>("[data-tool]:not(:disabled)"),
    );
    const index = buttons.indexOf(target);
    const next = { ArrowRight: index + 1, ArrowLeft: index - 1, Home: 0, End: -1 }[event.key];
    if (next === undefined) return;
    event.preventDefault();
    buttons.at(next % buttons.length)?.focus();
  };

  const common = (tool: ToolKey) => ({
    tool,
    current: tool === rovingTool,
    disabled: tool === "source" ? disabled : formatDisabled,
    onFocus: setFocusTool,
  });
  const format = (tool: Tool, label: string) => (
    <ToolButton
      {...common(tool)}
      label={label}
      pressed={caret.formats.has(tool)}
      onClick={() => {
        onFormat(tool);
      }}
    >
      <ComposerIcon d={EDITOR_ICONS[tool]} size={16} />
    </ToolButton>
  );
  const divider = <span className="rte__divider" aria-hidden="true" />;

  return (
    <div role="toolbar" aria-label={t("editor.toolbar")} className="rte__bar" onKeyDown={keyDown}>
      {format("bold", t("editor.bold"))}
      {format("italic", t("editor.italic"))}
      {format("underline", t("editor.underline"))}
      {format("strike", t("editor.strike"))}
      {divider}
      <ToolButton
        {...common("color")}
        label={t("editor.color")}
        expanded={popup === "color"}
        wide
        onClick={onColor}
      >
        <span className="rte__color-icon">
          <ComposerIcon d={EDITOR_ICONS.color} size={16} />
          <span className="rte__color-bar" style={{ background: caret.color ?? "currentColor" }} />
        </span>
        <ComposerIcon d={EDITOR_ICONS.chevron} size={12} />
      </ToolButton>
      {divider}
      <ToolButton
        {...common("link")}
        label={t("editor.link")}
        expanded={popup === "link"}
        active={caret.link !== null}
        onClick={onLink}
      >
        <ComposerIcon d={EDITOR_ICONS.link} size={16} />
      </ToolButton>
      <ToolButton {...common("image")} label={t("editor.image")} onClick={onImage}>
        <ComposerIcon d={EDITOR_ICONS.image} size={16} />
      </ToolButton>
      {divider}
      {format("bullets", t("editor.bullets"))}
      {format("numbers", t("editor.numbers"))}
      <span className="rte__spacer" />
      <button
        type="button"
        className="rte__source-toggle"
        aria-pressed={inSource}
        data-tooltip={inSource ? t("editor.sourceHide") : t("editor.sourceShow")}
        data-tooltip-placement="bottom"
        data-tool="source"
        tabIndex={rovingTool === "source" ? 0 : -1}
        disabled={disabled}
        onFocus={() => {
          setFocusTool("source");
        }}
        onClick={onSource}
      >
        <ComposerIcon d={EDITOR_ICONS.source} size={16} />
        <span>{t("editor.source")}</span>
      </button>
    </div>
  );
}
