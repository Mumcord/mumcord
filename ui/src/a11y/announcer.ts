/**
 * Screen-reader announcements through ARIA live regions. Features call `announce()` for events
 * that are not otherwise visible to assistive technology (someone starts talking, joins, …);
 * `<LiveRegions />` renders the regions once at the app root.
 */
export type Politeness = "polite" | "assertive";

export interface Announcement {
  message: string;
  politeness: Politeness;
}

type Listener = (announcement: Announcement) => void;

const listeners = new Set<Listener>();

/**
 * Announces `message`. Use "polite" (default) for status updates and "assertive" only for
 * urgent problems such as a lost connection.
 */
export function announce(message: string, politeness: Politeness = "polite"): void {
  for (const listener of listeners) listener({ message, politeness });
}

export function subscribeAnnouncements(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
