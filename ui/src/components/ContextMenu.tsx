/**
 * Small context menu (chat images and messages, theme cards): positioned at the pointer, kept
 * inside the window, arrow keys between items, Escape or Tab closes it and returns the focus.
 */
import { useEffect, useLayoutEffect, useRef, type KeyboardEvent } from "react";
import { createPortal } from "react-dom";

/** Gap kept between the menu and the window edge. */
const EDGE = 8;

export interface MenuPosition {
  /** Viewport position of the pointer (or of the item for keyboard use). */
  x: number;
  y: number;
  /** Gets the focus back when the menu closes. */
  opener: HTMLElement | null;
}

export interface MenuItem {
  label: string;
  onSelect: () => void;
}

export interface ContextMenuProps {
  menu: MenuPosition;
  /** Accessible name of the menu. */
  label: string;
  items: readonly MenuItem[];
  onClose: () => void;
}

/** The ranges of the page selection, to put back after a focus change collapsed them. */
function saveSelection(): Range[] {
  const selection = window.getSelection();
  if (!selection) return [];
  return Array.from({ length: selection.rangeCount }, (_, i) =>
    selection.getRangeAt(i).cloneRange(),
  );
}

function restoreSelection(ranges: readonly Range[]) {
  const selection = window.getSelection();
  if (!selection || ranges.length === 0) return;
  selection.removeAllRanges();
  for (const range of ranges) selection.addRange(range);
}

export function ContextMenu({ menu, label, items, onClose }: ContextMenuProps) {
  const ref = useRef<HTMLDivElement>(null);
  // Focusing a menu button (or the opener again on close) collapses the selection in some
  // webviews, which made selected text look unselected while the menu was open (#135).
  const selection = useRef<Range[]>([]);

  const close = (returnFocus: boolean) => {
    onClose();
    if (returnFocus && menu.opener?.isConnected) menu.opener.focus();
  };

  useEffect(() => {
    const dismiss = () => {
      onClose();
    };
    const onPointerDown = (event: PointerEvent) => {
      if (!(event.target instanceof Node && ref.current?.contains(event.target))) dismiss();
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("blur", dismiss);
    window.addEventListener("resize", dismiss);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("blur", dismiss);
      window.removeEventListener("resize", dismiss);
    };
  }, [onClose]);

  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const left = Math.max(EDGE, Math.min(menu.x, window.innerWidth - element.offsetWidth - EDGE));
    const top = Math.max(EDGE, Math.min(menu.y, window.innerHeight - element.offsetHeight - EDGE));
    element.style.left = `${String(left)}px`;
    element.style.top = `${String(top)}px`;
    selection.current = saveSelection();
    element.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
    restoreSelection(selection.current);
  }, [menu]);

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const elements = [...(ref.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])];
    const index = elements.findIndex((item) => item === document.activeElement);
    const focusAt = (next: number) => {
      elements[(next + elements.length) % elements.length]?.focus();
    };
    switch (event.key) {
      case "ArrowDown":
        focusAt(index + 1);
        break;
      case "ArrowUp":
        focusAt(index - 1);
        break;
      case "Home":
        focusAt(0);
        break;
      case "End":
        focusAt(elements.length - 1);
        break;
      case "Escape":
      case "Tab":
        close(true);
        break;
      default:
        return;
    }
    event.preventDefault();
    event.stopPropagation();
  }

  return createPortal(
    <div
      ref={ref}
      className="server-menu"
      role="menu"
      aria-label={label}
      tabIndex={-1}
      onKeyDown={onKeyDown}
    >
      {items.map((item) => (
        <button
          key={item.label}
          type="button"
          role="menuitem"
          tabIndex={-1}
          className="server-menu__item"
          onClick={() => {
            close(true);
            restoreSelection(selection.current);
            item.onSelect();
          }}
        >
          {item.label}
        </button>
      ))}
    </div>,
    document.body,
  );
}
