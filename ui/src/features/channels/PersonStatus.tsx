/**
 * Mute/deafen/listener/unregistered icons for a person; the spoken equivalent is `useStatusText()`.
 */
import type { User } from "../../api";
import { isDeafened, isMuted } from "./statusText";

type IconName = "mic-off" | "headphones-off" | "ear";

const PATHS: Record<IconName, string> = {
  "mic-off":
    "M12 2a3 3 0 0 0-3 3v6a3 3 0 0 0 4.6 2.5M15 9V5a3 3 0 0 0-.9-2.1M5 10v1a7 7 0 0 0 11.5 5.4M19 10v1a7 7 0 0 1-.4 2.3M12 18v4M3 3l18 18",
  "headphones-off":
    "M3 18v-6a9 9 0 0 1 14.4-7.2M21 12v6M21 18a2 2 0 0 1-2 2h-1v-6h3M3 18a2 2 0 0 0 2 2h1v-6H3M3 3l18 18",
  ear: "M6 8.5a6 6 0 1 1 12 0c0 3-2 4.5-3 5.5s-1.5 2-1.5 3.5a3 3 0 0 1-6 0M9 8.5a3 3 0 0 1 6 0",
};

export function Icon({ name }: { name: IconName }) {
  return (
    <svg
      className="person-icon"
      viewBox="0 0 24 24"
      width="16"
      height="16"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      <path d={PATHS[name]} />
    </svg>
  );
}

/** Visual status icons; the text equivalent comes from `useStatusText`. */
export function StatusIcons({ user, listener = false }: { user: User; listener?: boolean }) {
  if (listener) {
    return (
      <span className="person-icons">
        <Icon name="ear" />
      </span>
    );
  }
  return (
    <span className="person-icons">
      {!user.registered && (
        <span className="person-icons__unregistered" aria-hidden="true">
          ○
        </span>
      )}
      {isDeafened(user) ? <Icon name="headphones-off" /> : isMuted(user) && <Icon name="mic-off" />}
    </span>
  );
}
