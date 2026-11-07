import { Fragment, useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";

export interface MenuItem {
  id: string;
  label: string;
  danger?: boolean;
  /** Makes the item a switch (menuitemcheckbox) with this state. */
  checked?: boolean;
  /** Shown but not usable yet; stays focusable so screen readers can find it. */
  disabled?: boolean;
  /** Decorative icon before the label. */
  icon?: ReactNode;
  /** Short state at the end of the row, e.g. "Following". */
  hint?: string;
  /** Draws a divider above the item. */
  separatorBefore?: boolean;
  /**
   * Makes the item open a sub-menu with these items (e.g. "Admin ▸", Epic 4 FR-10.6) instead of
   * running `onSelect`; Enter, Space or → open it, ← or Escape go back.
   */
  submenu?: MenuItem[];
  onSelect: () => void;
}

interface ServerMenuProps {
  label: string;
  items: MenuItem[];
  position: { x: number; y: number };
  /** Closes the menu; `returnFocus` is false when the user clicked elsewhere. */
  onClose: (returnFocus: boolean) => void;
  /** A sub-menu: ← closes it like Escape, and it leaves outside presses to its parent. */
  nested?: boolean;
}

interface OpenSubmenu {
  id: string;
  position: { x: number; y: number };
  opener: HTMLElement;
}

const ITEM_ROLES = '[role="menuitem"], [role="menuitemcheckbox"]';
/** This menu's own items, without those of an open sub-menu inside it. */
const OWN_ITEMS = ':scope > [role="menuitem"], :scope > [role="menuitemcheckbox"]';

/** Context menu for a server icon: arrow keys move, Enter selects, Escape/Tab close. */
export function ServerMenu({ label, items, position, onClose, nested = false }: ServerMenuProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [submenu, setSubmenu] = useState<OpenSubmenu | null>(null);
  const openItem = items.find((item) => item.id === submenu?.id);

  const openSubmenu = (item: MenuItem, opener: HTMLElement) => {
    const rect = opener.getBoundingClientRect();
    setSubmenu({ id: item.id, position: { x: rect.right + 4, y: rect.top }, opener });
  };

  useEffect(() => {
    ref.current?.querySelector<HTMLElement>(ITEM_ROLES)?.focus();
    // The parent menu decides about presses outside; a sub-menu sits inside its parent's element.
    if (nested) return;
    const onPointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node && !ref.current?.contains(event.target)) onClose(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
    };
  }, [onClose, nested]);

  const onKeyDown = (event: KeyboardEvent) => {
    // Only this menu's own items; an open sub-menu handles its keys itself.
    const menuItems = [...(ref.current?.querySelectorAll<HTMLElement>(OWN_ITEMS) ?? [])];
    const index = menuItems.findIndex((item) => item === document.activeElement);
    const focusAt = (next: number) => {
      menuItems[(next + menuItems.length) % menuItems.length]?.focus();
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
        focusAt(menuItems.length - 1);
        break;
      case "ArrowRight": {
        const item = items[index];
        const element = menuItems[index];
        if (item?.submenu === undefined || element === undefined) return;
        openSubmenu(item, element);
        break;
      }
      case "ArrowLeft":
        if (!nested) return;
        onClose(true);
        break;
      case "Escape":
      case "Tab":
        onClose(true);
        break;
      default:
        return;
    }
    event.preventDefault();
    event.stopPropagation();
  };

  return (
    <div
      ref={ref}
      className="server-menu"
      role="menu"
      aria-label={label}
      tabIndex={-1}
      style={{ left: position.x, top: position.y }}
      onKeyDown={onKeyDown}
    >
      {items.map((item) => (
        <Fragment key={item.id}>
          {item.separatorBefore && <div role="separator" className="server-menu__separator" />}
          <button
            type="button"
            role={item.checked === undefined ? "menuitem" : "menuitemcheckbox"}
            aria-checked={item.checked}
            aria-disabled={item.disabled}
            aria-haspopup={item.submenu === undefined ? undefined : "menu"}
            aria-expanded={item.submenu === undefined ? undefined : submenu?.id === item.id}
            tabIndex={-1}
            className={
              item.danger ? "server-menu__item server-menu__item--danger" : "server-menu__item"
            }
            onClick={(event) => {
              if (item.disabled) return;
              if (item.submenu !== undefined) openSubmenu(item, event.currentTarget);
              else item.onSelect();
            }}
          >
            {item.checked === undefined ? null : (
              <span className="server-menu__check" aria-hidden="true">
                {item.checked ? "✓" : ""}
              </span>
            )}
            {item.icon && (
              <span className="server-menu__icon" aria-hidden="true">
                {item.icon}
              </span>
            )}
            {item.label}
            {item.hint && <span className="server-menu__end">{item.hint}</span>}
            {item.submenu !== undefined && (
              <svg
                className="server-menu__submenu"
                viewBox="0 0 24 24"
                width="16"
                height="16"
                aria-hidden="true"
                focusable="false"
              >
                <path d="M9 6l6 6-6 6" fill="none" stroke="currentColor" strokeWidth="2" />
              </svg>
            )}
          </button>
        </Fragment>
      ))}
      {submenu !== null && openItem?.submenu !== undefined && (
        <ServerMenu
          nested
          label={openItem.label}
          items={openItem.submenu}
          position={submenu.position}
          onClose={(returnFocus) => {
            const { opener } = submenu;
            setSubmenu(null);
            if (returnFocus) opener.focus();
            else onClose(false);
          }}
        />
      )}
    </div>
  );
}
