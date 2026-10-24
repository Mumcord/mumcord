/**
 * Polite screen-reader announcements for joins, leaves and talking in the own voice channel.
 * Rate-limited so a busy channel doesn't flood the screen reader.
 */
import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { selfUser, subscribeServerChanges } from "./serverStore";

/** Minimum time between any two announcements, and between talking announcements per person. */
export const GLOBAL_INTERVAL_MS = 1500;
export const TALK_INTERVAL_MS = 10_000;

export class AnnouncementLimiter {
  private lastAny = -Infinity;
  private readonly lastTalk = new Map<number, number>();

  /** Whether an announcement may be spoken now; records it if so. */
  allow(now: number, talkingSession?: number): boolean {
    if (now - this.lastAny < GLOBAL_INTERVAL_MS) return false;
    if (talkingSession !== undefined) {
      const last = this.lastTalk.get(talkingSession) ?? -Infinity;
      if (now - last < TALK_INTERVAL_MS) return false;
      this.lastTalk.set(talkingSession, now);
    }
    this.lastAny = now;
    return true;
  }
}

/** Subscribes to server changes and announces them while mounted. */
export function useServerAnnouncements(): void {
  const { t } = useTranslation("channels");

  useEffect(() => {
    const limiter = new AnnouncementLimiter();
    return subscribeServerChanges((change, state) => {
      const self = selfUser(state);
      if (!self) return;
      const now = Date.now();
      let message: string | undefined;
      let talkingSession: number | undefined;
      if (change.kind === "userUpserted" && change.user.session !== self.session) {
        const wasHere = change.previous?.channelId === self.channelId;
        const isHere = change.user.channelId === self.channelId;
        if (isHere && !wasHere) message = t("announce.joined", { name: change.user.name });
        if (wasHere && !isHere) message = t("announce.left", { name: change.user.name });
      } else if (change.kind === "userRemoved" && change.user.channelId === self.channelId) {
        message = t("announce.left", { name: change.user.name });
      } else if (change.kind === "talk" && change.talking && change.session !== self.session) {
        const user = state.users.get(change.session);
        if (user?.channelId === self.channelId) {
          message = t("announce.talking", { name: user.name });
          talkingSession = user.session;
        }
      }
      if (message && limiter.allow(now, talkingSession)) announce(message, "polite");
    });
  }, [t]);
}
