/** How long a scrollbar stays visible after the last scroll step. */
export const SCROLL_IDLE_MS = 1000;

/**
 * Marks an element with `data-scrolling` while it scrolls, so global.css shows its scrollbar only
 * then and hides it when idle (#875). CSS alone cannot tell that an element is scrolling. A data
 * attribute rather than a class, because React rewrites `className` on every render.
 */
export function installScrollActivity(doc: Document = document): () => void {
  const timers = new Map<Element, ReturnType<typeof setTimeout>>();
  const onScroll = (event: Event) => {
    const el = event.target;
    if (!(el instanceof Element)) return;
    const pending = timers.get(el);
    if (pending === undefined) el.setAttribute("data-scrolling", "");
    else clearTimeout(pending);
    timers.set(
      el,
      setTimeout(() => {
        timers.delete(el);
        el.removeAttribute("data-scrolling");
      }, SCROLL_IDLE_MS),
    );
  };
  // Scroll events do not bubble; capturing on the document sees them for every element.
  doc.addEventListener("scroll", onScroll, { capture: true, passive: true });
  return () => {
    doc.removeEventListener("scroll", onScroll, { capture: true });
    for (const [el, timer] of timers) {
      clearTimeout(timer);
      el.removeAttribute("data-scrolling");
    }
    timers.clear();
  };
}
