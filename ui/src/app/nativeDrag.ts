// WebKitGTK copies the whole `data:` URL of a dragged <img> into the drag payload. For the
// multi-MB GIFs of the chat that freezes the app, and nothing in Mumcord accepts such a drop.
// Text selections and input contents keep dragging: only images (and data: links) are blocked.
export function shouldBlockNativeDrag(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  if (target.closest("img, svg image")) return true;
  const link = target.closest("a[href]");
  return link !== null && (link.getAttribute("href") ?? "").startsWith("data:");
}

/**
 * True for drags that carry files or links from outside the page (file manager, browser).
 * WebKitGTK reports file drags only as `text/uri-list` (`file://` links), without "Files".
 */
export function isFileDrag(dataTransfer: DataTransfer | null): boolean {
  if (dataTransfer === null) return false;
  const types = Array.from(dataTransfer.types);
  return types.includes("Files") || types.includes("text/uri-list");
}

export function installNativeDragGuard(doc: Document = document): () => void {
  const onDragStart = (event: DragEvent) => {
    if (shouldBlockNativeDrag(event.target)) event.preventDefault();
  };
  // A file or link dropped where the page does not take it makes the webview navigate to it,
  // replacing the whole app. Accepting every such drag at window level (and then ignoring the
  // drop) prevents that; the message box handles its own drops before this runs.
  const onFileDrag = (event: DragEvent) => {
    if (isFileDrag(event.dataTransfer)) event.preventDefault();
  };
  const view = doc.defaultView;
  doc.addEventListener("dragstart", onDragStart, true);
  view?.addEventListener("dragover", onFileDrag);
  view?.addEventListener("drop", onFileDrag);
  return () => {
    doc.removeEventListener("dragstart", onDragStart, true);
    view?.removeEventListener("dragover", onFileDrag);
    view?.removeEventListener("drop", onFileDrag);
  };
}
