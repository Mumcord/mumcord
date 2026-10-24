import { useEffect, useRef, type KeyboardEvent } from "react";

export interface MenuItem {
  id: string;
  label: string;
  danger?: boolean;
  onSelect: () => void;
}

interface ServerMenuProps {
  label: string;
  items: MenuItem[];
  position: { x: number; y: number };
  /** Closes the menu; `returnFocus` is false when the user clicked elsewhere. */
  onClose: (returnFocus: boolean) => void;
}

/** Context menu for a server icon: arrow keys move, Enter selects, Escape/Tab close. */
export function ServerMenu({ label, items, position, onClose }: ServerMenuProps) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    ref.current?.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
    const onPointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node && !ref.current?.contains(event.target)) onClose(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
    };
  }, [onClose]);

  const onKeyDown = (event: KeyboardEvent) => {
    const menuItems = [...(ref.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])];
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
        <button
          key={item.id}
          type="button"
          role="menuitem"
          tabIndex={-1}
          className={
            item.danger ? "server-menu__item server-menu__item--danger" : "server-menu__item"
          }
          onClick={item.onSelect}
        >
          {item.label}
        </button>
      ))}
    </div>
  );
}
