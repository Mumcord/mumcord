/**
 * Speaks new messages of the open conversation without flooding the screen reader: the first
 * message is read out, further ones within the quiet period are summed up afterwards
 * ("3 new messages").
 */
export const QUIET_MS = 3000;

export interface MessageAnnouncer {
  /** A new message arrived; `text` is what to say when it is the only one. */
  push(text: string): void;
  dispose(): void;
}

export function createMessageAnnouncer(
  speak: (text: string) => void,
  summary: (count: number) => string,
  quietMs = QUIET_MS,
): MessageAnnouncer {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let pending = 0;

  function flush(): void {
    if (pending === 0) {
      timer = undefined;
      return;
    }
    speak(summary(pending));
    pending = 0;
    timer = setTimeout(flush, quietMs);
  }

  return {
    push(text) {
      if (timer === undefined) {
        speak(text);
        timer = setTimeout(flush, quietMs);
      } else {
        pending += 1;
      }
    },
    dispose() {
      clearTimeout(timer);
      timer = undefined;
      pending = 0;
    },
  };
}
