import { useEffect, useState } from "react";
import { subscribeAnnouncements, type Politeness } from "./announcer";

// Screen readers ignore a region whose text does not change, so a repeated message gets an
// alternating invisible suffix to be announced again.
const REPEAT_MARK = " ";

/** Hidden live regions that speak `announce()` messages. Mount exactly once. */
export function LiveRegions() {
  const [messages, setMessages] = useState<Record<Politeness, string>>({
    polite: "",
    assertive: "",
  });

  useEffect(
    () =>
      subscribeAnnouncements(({ message, politeness }) => {
        setMessages((current) => ({
          ...current,
          [politeness]: current[politeness] === message ? `${message}${REPEAT_MARK}` : message,
        }));
      }),
    [],
  );

  return (
    <div className="visually-hidden">
      <div aria-live="polite" aria-atomic="true" data-testid="live-polite">
        {messages.polite}
      </div>
      <div aria-live="assertive" aria-atomic="true" data-testid="live-assertive">
        {messages.assertive}
      </div>
    </div>
  );
}
