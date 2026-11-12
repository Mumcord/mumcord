import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import { normalizeColor } from "../chat/sanitize";
import { normalizeLink } from "./commentHtml";
import { SWATCHES } from "./richEditorIcons";

interface PopoverProps {
  label: string;
  className: string;
  style: CSSProperties;
  onClose: () => void;
  children: ReactNode;
}

/** A small dialog under a toolbar button; Escape closes it without closing the window. */
function Popover({ label, className, style, onClose, children }: PopoverProps) {
  const box = useRef<HTMLDivElement>(null);
  const close = useRef(onClose);
  useEffect(() => {
    close.current = onClose;
  });

  // Opens under its button, but moves left where it would stick out of the editor.
  useLayoutEffect(() => {
    const element = box.current;
    const parent = element?.offsetParent;
    if (!element || !(parent instanceof HTMLElement)) return;
    const overflow = element.offsetLeft + element.offsetWidth - parent.clientWidth;
    if (overflow > 0)
      element.style.left = `${String(Math.max(0, element.offsetLeft - overflow))}px`;
  }, [style]);

  useEffect(() => {
    const element = box.current;
    if (!element) return;
    const keyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      close.current();
    };
    element.addEventListener("keydown", keyDown);
    return () => {
      element.removeEventListener("keydown", keyDown);
    };
  }, []);

  return (
    <div
      ref={box}
      className={`rte-pop ${className}`}
      role="dialog"
      aria-label={label}
      style={style}
    >
      {children}
    </div>
  );
}

/** Swatches per row; arrow up and down move by a whole row. */
const COLUMNS = 6;

interface ColorPopoverProps {
  current: string | null;
  style: CSSProperties;
  onPick: (color: string | null) => void;
  onClose: () => void;
}

/** Text colour: "default" (no colour), a palette and a custom hex colour (issue #666). */
export function ColorPopover({ current, style, onPick, onClose }: ColorPopoverProps) {
  const { t } = useTranslation("userpanel");
  const swatches = useRef<HTMLDivElement>(null);
  const picker = useRef<HTMLInputElement>(null);
  const [custom, setCustom] = useState(current ?? "");
  const customColor = normalizeColor(custom);
  const choices: { value: string | null; label: string }[] = [
    { value: null, label: t("editor.defaultColor") },
    ...SWATCHES.map((s) => ({ value: s.value, label: t(`editor.colorNames.${s.name}`) })),
  ];
  // A custom colour checks no swatch; focus then starts on "default".
  const checked = choices.findIndex((c) => c.value === current);
  const focusable = Math.max(0, checked);

  useEffect(() => {
    swatches.current?.querySelectorAll("button")[focusable]?.focus();
    // Focus moves in once, when the palette opens.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const move = (event: KeyboardEvent) => {
    const step = { ArrowRight: 1, ArrowLeft: -1, ArrowDown: COLUMNS, ArrowUp: -COLUMNS }[event.key];
    if (step === undefined) return;
    event.preventDefault();
    const buttons = Array.from(swatches.current?.querySelectorAll("button") ?? []);
    const index = buttons.indexOf(event.target as HTMLButtonElement);
    const next = Math.min(buttons.length - 1, Math.max(0, index + step));
    buttons[next]?.focus();
  };

  return (
    <Popover label={t("editor.color")} className="rte-pop--color" style={style} onClose={onClose}>
      <div
        ref={swatches}
        className="rte-pop__swatches"
        role="radiogroup"
        aria-label={t("editor.colors")}
      >
        {choices.map((choice, index) => (
          <button
            key={choice.value ?? "default"}
            type="button"
            role="radio"
            aria-checked={index === checked}
            tabIndex={index === focusable ? 0 : -1}
            className={`rte-pop__swatch${choice.value === null ? " rte-pop__swatch--none" : ""}`}
            aria-label={choice.label}
            data-tooltip={choice.label}
            style={choice.value === null ? undefined : { background: choice.value }}
            onKeyDown={move}
            onClick={() => {
              onPick(choice.value);
            }}
          />
        ))}
      </div>
      <div className="rte-pop__custom">
        <span className="rte-pop__label">{t("editor.custom")}</span>
        <span
          className="rte-pop__chip"
          style={{ background: customColor ?? "transparent" }}
          aria-hidden="true"
        />
        <input
          type="text"
          aria-label={t("editor.customHex")}
          aria-invalid={custom !== "" && customColor === null}
          placeholder="#rrggbb"
          spellCheck={false}
          value={custom}
          onChange={(e) => {
            setCustom(e.target.value);
          }}
          onKeyDown={(e) => {
            if (e.key !== "Enter") return;
            e.preventDefault();
            if (customColor !== null) onPick(customColor);
          }}
        />
        <button
          type="button"
          className="button"
          onClick={() => {
            picker.current?.click();
          }}
        >
          {t("editor.pick")}
        </button>
        <input
          ref={picker}
          type="color"
          className="rte-pop__picker"
          tabIndex={-1}
          aria-hidden="true"
          value={customColor ?? "#000000"}
          onChange={(e) => {
            setCustom(e.target.value);
            onPick(e.target.value);
          }}
        />
      </div>
    </Popover>
  );
}

interface LinkPopoverProps {
  text: string;
  address: string;
  /** Editing an existing link: offers Remove link. */
  editing: boolean;
  style: CSSProperties;
  onApply: (text: string, href: string) => void;
  onRemove: () => void;
  onClose: () => void;
}

/** Link text and address in place of a prompt (issue #666). */
export function LinkPopover({
  text: initialText,
  address: initialAddress,
  editing,
  style,
  onApply,
  onRemove,
  onClose,
}: LinkPopoverProps) {
  const { t } = useTranslation(["userpanel", "common"]);
  const [text, setText] = useState(initialText);
  const [address, setAddress] = useState(initialAddress);
  const [invalid, setInvalid] = useState(false);
  const addressInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    addressInput.current?.focus();
  }, []);

  const apply = () => {
    const href = normalizeLink(address);
    if (href === null) {
      setInvalid(true);
      addressInput.current?.focus();
      return;
    }
    onApply(text, href);
  };
  const enter = (event: KeyboardEvent) => {
    if (event.key !== "Enter") return;
    event.preventDefault();
    apply();
  };

  return (
    <Popover
      label={editing ? t("editor.editLink") : t("editor.link")}
      className="rte-pop--link"
      style={style}
      onClose={onClose}
    >
      <label className="rte-pop__field">
        <span>{t("editor.linkText")}</span>
        <input
          type="text"
          value={text}
          onChange={(e) => {
            setText(e.target.value);
          }}
          onKeyDown={enter}
        />
      </label>
      <label className="rte-pop__field">
        <span>{t("editor.linkAddress")}</span>
        <input
          ref={addressInput}
          type="text"
          placeholder="https://"
          spellCheck={false}
          value={address}
          aria-invalid={invalid}
          onChange={(e) => {
            setAddress(e.target.value);
            setInvalid(false);
          }}
          onKeyDown={enter}
        />
      </label>
      {invalid && (
        <p className="rte-pop__error" role="alert">
          {t("editor.linkInvalid")}
        </p>
      )}
      <div className="rte-pop__actions">
        {editing && (
          <button type="button" className="button button--link rte-pop__remove" onClick={onRemove}>
            {t("editor.removeLink")}
          </button>
        )}
        <span className="rte__spacer" />
        <button type="button" className="button" onClick={onClose}>
          {t("common:cancel")}
        </button>
        <button type="button" className="button button--primary" onClick={apply}>
          {t("editor.apply")}
        </button>
      </div>
    </Popover>
  );
}
