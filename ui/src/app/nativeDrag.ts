// WebKitGTK copies the whole `data:` URL of a dragged <img> into the drag payload. For the
// multi-MB GIFs of the chat that freezes the app, and nothing in Mumcord accepts such a drop.
// Text selections and input contents keep dragging: only images (and data: links) are blocked.
export function shouldBlockNativeDrag(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  if (target.closest("img, svg image")) return true;
  const link = target.closest("a[href]");
  return link !== null && (link.getAttribute("href") ?? "").startsWith("data:");
}

export function installNativeDragGuard(doc: Document = document): () => void {
  const onDragStart = (event: DragEvent) => {
    if (shouldBlockNativeDrag(event.target)) event.preventDefault();
  };
  doc.addEventListener("dragstart", onDragStart, true);
  return () => {
    doc.removeEventListener("dragstart", onDragStart, true);
  };
}
