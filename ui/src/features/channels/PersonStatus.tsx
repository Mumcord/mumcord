/**
 * Mute/deafen/listener/unregistered icons for a person; the spoken equivalent is `useStatusText()`.
 */
import { useTranslation } from "react-i18next";
import type { User } from "../../api";
import { voiceState, type VoiceState } from "./statusText";

type IconName =
  "mic-off" | "headphones-off" | "ear" | "ban" | "volume-x" | "user-guest" | "user-check";

const PATHS: Record<IconName, string> = {
  "mic-off":
    "M12 2a3 3 0 0 0-3 3v6a3 3 0 0 0 4.6 2.5M15 9V5a3 3 0 0 0-.9-2.1M5 10v1a7 7 0 0 0 11.5 5.4M19 10v1a7 7 0 0 1-.4 2.3M12 18v4M3 3l18 18",
  "headphones-off":
    "M3 18v-6a9 9 0 0 1 14.4-7.2M21 12v6M21 18a2 2 0 0 1-2 2h-1v-6h3M3 18a2 2 0 0 0 2 2h1v-6H3M3 3l18 18",
  ear: "M6 8.5a6 6 0 1 1 12 0c0 3-2 4.5-3 5.5s-1.5 2-1.5 3.5a3 3 0 0 1-6 0M9 8.5a3 3 0 0 1 6 0",
  ban: "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18M5.6 5.6l12.8 12.8",
  "volume-x": "M11 5 6 9H3v6h3l5 4V5M22 9l-6 6M16 9l6 6",
  // Person outline; the guest variant dashes it so it reads as "no account".
  "user-check": "M12 4a4 4 0 1 0 0 8a4 4 0 1 0 0-8M4 21a8 8 0 0 1 16 0M15 17l1.8 1.8L20 15.5",
  "user-guest": "M12 4a4 4 0 1 0 0 8a4 4 0 1 0 0-8M4 21a8 8 0 0 1 16 0",
};

/** Icon and look of each voice state; what the server imposes gets the red "server" chip. */
const VOICE_ICONS: Record<VoiceState, { name: IconName; variant?: string }> = {
  serverDeafened: { name: "headphones-off", variant: "server" },
  deafened: { name: "headphones-off" },
  serverMuted: { name: "mic-off", variant: "server" },
  suppressed: { name: "ban", variant: "suppressed" },
  muted: { name: "mic-off" },
};

export function Icon({
  name,
  variant,
  tip,
}: {
  name: IconName;
  variant?: string | undefined;
  tip?: string | undefined;
}) {
  const svg = (
    <svg
      className={variant ? `person-icon person-icon--${variant}` : "person-icon"}
      data-icon={name}
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
  // A wrapper carries the native tooltip: SVG tooltips need a <title>, which would leak into text content.
  return tip ? (
    <span className="person-icon-slot" title={tip}>
      {svg}
    </span>
  ) : (
    svg
  );
}

/** The icon of a voice state without its look; the avatar stack's badge colours it itself. */
export function VoiceStateIcon({ state }: { state: VoiceState }) {
  return <Icon name={VOICE_ICONS[state].name} />;
}

/**
 * Visual status icons with a hover tooltip; the text equivalent comes from `useStatusText`.
 * Voice states come first so they line up; the registered/guest marker sits last.
 */
export function StatusIcons({ user, listener = false }: { user: User; listener?: boolean }) {
  const { t } = useTranslation("channels");
  if (listener) {
    return (
      <span className="person-icons">
        <Icon name="ear" tip={t("statusTip.listening")} />
      </span>
    );
  }
  const voice = voiceState(user);
  return (
    <span className="person-icons">
      {voice && (
        <Icon
          name={VOICE_ICONS[voice].name}
          variant={VOICE_ICONS[voice].variant}
          tip={t(`statusTip.${voice}`)}
        />
      )}
      {user.localMute && (
        <Icon name="volume-x" variant="local-muted" tip={t("statusTip.localMuted")} />
      )}
      {user.registered ? (
        <Icon name="user-check" variant="registered" tip={t("statusTip.registered")} />
      ) : (
        <Icon name="user-guest" variant="guest" tip={t("statusTip.unregistered")} />
      )}
    </span>
  );
}
