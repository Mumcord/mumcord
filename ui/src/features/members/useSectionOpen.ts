import { useState } from "react";

/** Open state per section, kept while the column is unmounted (narrow overlay). */
const remembered = new Map<string, boolean>();

const KEY_PREFIX = "mumcord.section.open.";

/**
 * Remembered per viewer across restarts; a UI convenience, so storage failures fall back to the
 * section's default instead of breaking the column.
 */
function readStored(id: string): boolean | undefined {
  try {
    const value = localStorage.getItem(KEY_PREFIX + id);
    return value === null ? undefined : value === "1";
  } catch {
    return undefined;
  }
}

function writeStored(id: string, open: boolean): void {
  try {
    localStorage.setItem(KEY_PREFIX + id, open ? "1" : "0");
  } catch (error) {
    console.error("Saving the section open state failed", error);
  }
}

/** Open state of the section `id`, remembered across remounts of the column and app restarts. */
export function useSectionOpen(
  id: string,
  defaultOpen: boolean,
): [boolean, (open: boolean) => void] {
  const [open, setOpenState] = useState(() => remembered.get(id) ?? readStored(id) ?? defaultOpen);
  const setOpen = (next: boolean) => {
    remembered.set(id, next);
    writeStored(id, next);
    setOpenState(next);
  };
  return [open, setOpen];
}
