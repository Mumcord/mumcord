/**
 * The "Admin ▸" entry of the channel and server menus (Epic 4, FR-10.6, issue #249): always a
 * sub-menu, after the normal items, with the destructive items last behind a divider. Items whose
 * right is missing are left out by the caller; without any item there is no Admin entry at all
 * (FR-10.2, FR-10.3).
 */
import type { MenuItem } from "../servers/ServerMenu";

/** The Admin entry for `items`, or `null` when the user may do none of them. */
export function adminEntry(label: string, items: readonly MenuItem[]): MenuItem | null {
  if (items.length === 0) return null;
  const safe = items.filter((item) => item.danger !== true);
  const risky = items
    .filter((item) => item.danger === true)
    .map((item, index) =>
      index === 0 && safe.length > 0 ? { ...item, separatorBefore: true } : item,
    );
  return {
    id: "admin",
    label,
    separatorBefore: true,
    submenu: [...safe.map((item) => ({ ...item, separatorBefore: false })), ...risky],
    // A sub-menu entry opens its items; selecting it does nothing else.
    onSelect: () => undefined,
  };
}
