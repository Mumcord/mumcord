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
  openProfile: (
    session: number,
    opener: HTMLElement | null,
    focusConnection?: boolean,
    serverId?: number,
  ) => void;
  profileCard: ReactNode;
} {
  const [open, setOpen] = useState<{
    session: number;
    serverId: number | undefined;
    opener: HTMLElement | null;
    focusConnection: boolean;
  } | null>(null);

  const openProfile = useCallback(
    (session: number, opener: HTMLElement | null, focusConnection = false, serverId?: number) => {
      setOpen({ session, serverId, opener, focusConnection });
    },
    [],
  );

  const close = useCallback(() => {
    setOpen(null);
  }, []);

  const profileCard =
    open === null ? null : (
      <ProfileCard
        session={open.session}
        serverId={open.serverId}
        onClose={close}
        returnFocus={open.opener}
        focusConnection={open.focusConnection}
        onSendPrivateMessage={onSendPrivateMessage}
      />
    );
  return { openProfile, profileCard };
}
