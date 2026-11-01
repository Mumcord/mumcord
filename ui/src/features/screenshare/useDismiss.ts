import { useEffect, type RefObject } from "react";

/**
 * Closes an open popover on Escape and on a click outside `refs` (the popover and the button that
 * toggles it, so that button's own click is not undone).
 */
export function useDismiss(
  refs: readonly RefObject<HTMLElement | null>[],
  open: boolean,
  close: () => void,
): void {
  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      // Escape here closes the popover only, not focus mode or full screen as well.
      event.stopPropagation();
      event.preventDefault();
      close();
    };
    const onPointer = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (!refs.some((ref) => ref.current?.contains(target) === true)) close();
    };
    document.addEventListener("keydown", onKey, true);
    document.addEventListener("pointerdown", onPointer);
    return () => {
      document.removeEventListener("keydown", onKey, true);
      document.removeEventListener("pointerdown", onPointer);
    };
  }, [refs, open, close]);
}
