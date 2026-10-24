/**
 * Opens the profile card for a person and returns focus to the element that opened it.
 * Usage: `const { openProfile, profileCard } = useProfileCard(onSendPrivateMessage)`; render
 * `profileCard` somewhere in the component.
 */
import { useCallback, useState, type ReactNode } from "react";
import { ProfileCard } from "./ProfileCard";
import { openPrivateConversation, type SendPrivateMessage } from "./privateMessage";

export function useProfileCard(
  onSendPrivateMessage: SendPrivateMessage = openPrivateConversation,
): {
  openProfile: (session: number, opener: HTMLElement | null) => void;
  profileCard: ReactNode;
} {
  const [open, setOpen] = useState<{ session: number; opener: HTMLElement | null } | null>(null);

  const openProfile = useCallback((session: number, opener: HTMLElement | null) => {
    setOpen({ session, opener });
  }, []);

  const close = useCallback(() => {
    setOpen(null);
  }, []);

  const profileCard =
    open === null ? null : (
      <ProfileCard
        session={open.session}
        onClose={close}
        returnFocus={open.opener}
        onSendPrivateMessage={onSendPrivateMessage}
      />
    );
  return { openProfile, profileCard };
}
