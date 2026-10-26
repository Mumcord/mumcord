import { useMemo, useState } from "react";
import type { ChatMessage } from "../../api";

interface MessageWindow {
  /** The messages to draw, oldest first. */
  shown: readonly ChatMessage[];
  /** Loaded messages before `shown` that are not drawn yet. */
  hidden: number;
  /** Draws up to `size` more of the hidden messages. */
  showOlder: () => void;
}

function firstShownId(messages: readonly ChatMessage[], size: number): string | null {
  return messages.length > size ? (messages[messages.length - size]?.id ?? null) : null;
}

/**
 * Only the newest `size` messages are drawn when the conversation `key` is shown; older ones that
 * are already loaded follow as the user scrolls up. Drawing hundreds of messages at once froze the
 * window when another server or conversation was shown (AC-2.17). Messages arriving later are
 * always drawn: the window only starts at an older message, it never ends early.
 */
export function useMessageWindow(
  messages: readonly ChatMessage[],
  key: string,
  size: number,
): MessageWindow {
  const [start, setStart] = useState(() => ({ key, firstId: firstShownId(messages, size) }));
  let current = start;
  if (start.key !== key) {
    current = { key, firstId: firstShownId(messages, size) };
    setStart(current);
  }
  const { firstId } = current;
  const index =
    firstId === null
      ? 0
      : Math.max(
          0,
          messages.findIndex((message) => message.id === firstId),
        );
  const shown = useMemo(() => (index === 0 ? messages : messages.slice(index)), [messages, index]);
  const showOlder = () => {
    const older = index - size;
    setStart({ key, firstId: older > 0 ? (messages[older]?.id ?? null) : null });
  };
  return { shown, hidden: index, showOlder };
}
