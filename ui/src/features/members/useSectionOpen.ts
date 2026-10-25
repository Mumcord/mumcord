import { useState } from "react";

/** Open state per section, kept while the column is unmounted (narrow overlay). */
const remembered = new Map<string, boolean>();

/** Open state of the section `id`, remembered across remounts of the column. */
export function useSectionOpen(
  id: string,
  defaultOpen: boolean,
): [boolean, (open: boolean) => void] {
  const [open, setOpenState] = useState(remembered.get(id) ?? defaultOpen);
  const setOpen = (next: boolean) => {
    remembered.set(id, next);
    setOpenState(next);
  };
  return [open, setOpen];
}
