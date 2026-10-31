type Listener = () => void;

/** Lets parts of the app outside the mounted `Updater` ask it to do something. */
function requestChannel(): { request: () => void; listen: (listener: Listener) => () => void } {
  const listeners = new Set<Listener>();
  return {
    request: () => {
      for (const listener of listeners) listener();
    },
    listen: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

const dialog = requestChannel();
const restart = requestChannel();

/** Asks the mounted update dialog to open, e.g. from the Updates section in Settings. */
export const openUpdateDialog = dialog.request;
export const onUpdateDialogRequest = dialog.listen;

/**
 * "Restart to update" outside the toast, e.g. in the notification centre. The `Updater` answers
 * it exactly like the toast's button, see `restartOrAsk`.
 */
export const requestUpdateRestart = restart.request;
export const onUpdateRestartRequest = restart.listen;

/**
 * What every "Restart to update" button does: restarting while connected first shows the "ready"
 * dialog, which explains what the restart does to the connections. Mumcord never ends a call
 * without asking.
 */
export function restartOrAsk(connectedServers: number, restartNow: Listener, ask: Listener): void {
  if (connectedServers > 0) ask();
  else restartNow();
}
