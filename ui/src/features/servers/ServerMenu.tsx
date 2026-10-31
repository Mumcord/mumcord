import { Fragment, useEffect, useRef, type KeyboardEvent, type ReactNode } from "react";

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
  onSelect: () => void;
}

interface ServerMenuProps {
  label: string;
  items: MenuItem[];
  position: { x: number; y: number };
  /** Closes the menu; `returnFocus` is false when the user clicked elsewhere. */
  onClose: (returnFocus: boolean) => void;
}

const ITEM_ROLES = '[role="menuitem"], [role="menuitemcheckbox"]';

/** Context menu for a server icon: arrow keys move, Enter selects, Escape/Tab close. */
export function ServerMenu({ label, items, position, onClose }: ServerMenuProps) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    ref.current?.querySelector<HTMLElement>(ITEM_ROLES)?.focus();
    const onPointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node && !ref.current?.contains(event.target)) onClose(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
    };
  }, [onClose]);

  const onKeyDown = (event: KeyboardEvent) => {
    const menuItems = [...(ref.current?.querySelectorAll<HTMLElement>(ITEM_ROLES) ?? [])];
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
            tabIndex={-1}
            className={
              item.danger ? "server-menu__item server-menu__item--danger" : "server-menu__item"
            }
            onClick={item.disabled ? undefined : item.onSelect}
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
          </button>
        </Fragment>
      ))}
    </div>
  );
}
