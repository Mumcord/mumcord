/**
 * Speaks camera changes in the room's polite live region (FR-8.69): who turned their camera on or
 * off, bundled over 2 s. The first look at a room says nothing; only later changes are spoken.
 */
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../../a11y";
import { createAnnouncer, type Announcer } from "./announcements";
import type { TileModel } from "./useRoomModel";

export function useCameraAnnouncements(tiles: readonly TileModel[]): void {
  const { t, i18n } = useTranslation("camera");
  const announcer = useRef<Announcer | null>(null);
  const previous = useRef<ReadonlyMap<number, string> | null>(null);
  const speak = useRef(t);
  useEffect(() => {
    speak.current = t;
  }, [t]);

  useEffect(() => {
    const list = new Intl.ListFormat(i18n.language, { type: "conjunction" });
    const created = createAnnouncer((bundles) => {
      for (const bundle of bundles) {
        const names = list.format(bundle.names);
        const key = bundle.on ? "announce.turnedOn" : "announce.turnedOff";
        announce(speak.current(key, { names, count: bundle.names.length }), "polite");
      }
    });
    announcer.current = created;
    return () => {
      created.cancel();
      if (announcer.current === created) announcer.current = null;
    };
  }, [i18n.language]);

  useEffect(() => {
    // Every camera of another person counts, also while it is hidden or still joining.
    const current = new Map<number, string>();
    for (const tile of tiles) if (tile.state !== "self") current.set(tile.session, tile.name);
    const before = previous.current;
    previous.current = current;
    if (before === null) return;
    for (const [session, name] of current)
      if (!before.has(session)) announcer.current?.push({ name, on: true });
    for (const [session, name] of before)
      if (!current.has(session)) announcer.current?.push({ name, on: false });
  }, [tiles]);
}
