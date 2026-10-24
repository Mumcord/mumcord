// The webview's built-in menu ("Inspect Element", "Reload", "Back") is not part of the app.
// It stays only where it is useful: editable fields (cut/copy/paste) and selected text (copy).
export function shouldSuppressNativeMenu(target: EventTarget | null, selection: string): boolean {
  if (selection.length > 0) return false;
  if (!(target instanceof Element)) return true;
  const editable = target.closest(
    "input, textarea, [contenteditable]:not([contenteditable='false'])",
  );
  return editable === null;
}

export function installNativeMenuSuppression(doc: Document = document): () => void {
  const onContextMenu = (event: MouseEvent) => {
    const selection = doc.defaultView?.getSelection()?.toString() ?? "";
    if (shouldSuppressNativeMenu(event.target, selection)) event.preventDefault();
  };
  doc.addEventListener("contextmenu", onContextMenu);
  return () => {
    doc.removeEventListener("contextmenu", onContextMenu);
  };
}
