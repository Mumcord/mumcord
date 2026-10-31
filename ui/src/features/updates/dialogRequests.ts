type Listener = () => void;

const listeners = new Set<Listener>();

/** Asks the mounted update dialog to open, e.g. from the Updates section in Settings. */
export function openUpdateDialog(): void {
  for (const listener of listeners) listener();
}

export function onUpdateDialogRequest(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
