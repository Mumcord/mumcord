/**
 * The playlist tab of the party panel next to the cinema view (issue #163, FR-3.125): a field to paste
 * a YouTube link, the Now playing card and the Up next list. Every row shows the thumbnail, a title of
 * up to two lines and who added it; rows are reordered by dragging (a line shows where the row
 * lands) or with Alt+Up / Alt+Down on the handle, and the ⋯ menu plays a row next or removes it.
 * Only the playing video has a known length, so only the Now playing card shows one.
 */
import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import type { PlaylistEntry, User } from "../../api";
import { useIsMacos } from "../../api/useIsMacos";
import { ContextMenu, type MenuPosition } from "../../components/ContextMenu";
import { safeImage } from "../chat/links";
import { Avatar } from "../members/Avatar";
import { useAvatars } from "../members/useAvatars";
import { clockTime } from "../media/format";
import { MediaIcon } from "../media/icons";
import { useKnownDuration } from "./knownDuration";
import { addToPlaylist, moveInPlaylist, removeFromPlaylist } from "./playlistActions";
import { useVideoPreview } from "./useVideoPreview";
import type { Adder, LiveParty } from "./watchPartyStore";
import { videoIdOfLink } from "./youtube";

interface PanelPlaylistProps {
  serverId: number;
  live: LiveParty;
  users: ReadonlyMap<number, User>;
  selfSession: number | null;
}

/** Where a dragged row would land: above or below the row under the pointer. */
type Drop = { entry: number; side: "before" | "after" } | null;

export function PanelPlaylist({ serverId, live, users, selfSession }: PanelPlaylistProps) {
  const { t } = useTranslation("watchparty");
  const { t: tc } = useTranslation("common");
  const party = live.party;
  const entries = party.playlist;
  const hintId = useId();
  const [dragged, setDragged] = useState<number | null>(null);
  const [drop, setDrop] = useState<Drop>(null);
  // The entry to keep focused after the list re-renders in its new order.
  const [focusEntry, setFocusEntry] = useState<number | null>(null);
  const list = useRef<HTMLOListElement>(null);

  useEffect(() => {
    if (focusEntry === null) return;
    list.current?.querySelector<HTMLElement>(`[data-entry="${String(focusEntry)}"]`)?.focus();
  }, [focusEntry, entries]);

  const nameOf = (adder: Adder) =>
    adder.session === selfSession
      ? t("you")
      : (users.get(adder.session)?.name ?? adder.name ?? t("someone"));

  const move = (entry: PlaylistEntry, index: number) => {
    if (index < 0 || index >= entries.length) return;
    moveInPlaylist(serverId, entry.id, index, t("playlist.moveFailed"));
  };

  const onMoveKey = (event: KeyboardEvent<HTMLElement>, entry: PlaylistEntry, index: number) => {
    if (!event.altKey || (event.key !== "ArrowUp" && event.key !== "ArrowDown")) return;
    event.preventDefault();
    const target = event.key === "ArrowUp" ? index - 1 : index + 1;
    if (target < 0 || target >= entries.length) return;
    setFocusEntry(entry.id);
    move(entry, target);
    announce(t("playlist.moved", { position: target + 1 }), "polite");
  };

  const endDrag = () => {
    setDragged(null);
    setDrop(null);
  };

  return (
    <section className="party-playlist" aria-label={t("playlist.title")}>
      <PasteField serverId={serverId} />
      <h3 className="media-panel__heading">{tc("media.nowPlaying")}</h3>
      <NowPlaying
        serverId={serverId}
        videoId={party.videoId}
        adder={live.currentAdder}
        name={live.currentAdder === null ? "" : nameOf(live.currentAdder)}
        users={users}
      />
      <h3 className="media-panel__heading">{t("upNextHeading")}</h3>
      {entries.length === 0 ? (
        <p className="party-playlist__empty">{t("playlist.emptyPaste")}</p>
      ) : (
        <>
          <p id={hintId} className="visually-hidden">
            {t("playlist.moveHint")}
          </p>
          <ol ref={list} className="party-playlist__list">
            {entries.map((entry, index) => (
              // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- dragging is the pointer way to reorder; the handle button moves entries by keyboard
              <li
                key={entry.id}
                className="party-entry"
                data-dragged={dragged === entry.id || undefined}
                data-drop={drop?.entry === entry.id ? drop.side : undefined}
                draggable
                onDragStart={(event) => {
                  event.dataTransfer.effectAllowed = "move";
                  event.dataTransfer.setData("text/plain", String(entry.id));
                  setDragged(entry.id);
                }}
                onDragEnd={endDrag}
                onDragOver={(event) => {
                  if (dragged === null) return;
                  event.preventDefault();
                  const from = entries.findIndex((e) => e.id === dragged);
                  const side = from < index ? "after" : "before";
                  if (dragged === entry.id) setDrop(null);
                  else if (drop?.entry !== entry.id || drop.side !== side)
                    setDrop({ entry: entry.id, side });
                }}
                onDrop={(event) => {
                  event.preventDefault();
                  const from = entries.find((e) => e.id === dragged);
                  endDrag();
                  if (from !== undefined && from.id !== entry.id) move(from, index);
                }}
              >
                <EntryRow
                  serverId={serverId}
                  entry={entry}
                  name={nameOf({ session: entry.addedBy, name: entry.addedByName })}
                  adder={users.get(entry.addedBy) ?? null}
                  first={index === 0}
                  hintId={hintId}
                  onMoveKey={(event) => {
                    onMoveKey(event, entry, index);
                  }}
                  onPlayNext={() => {
                    move(entry, 0);
                  }}
                />
              </li>
            ))}
          </ol>
        </>
      )}
    </section>
  );
}

/** "Paste a YouTube link": pasting a link adds it at once, Enter adds what was typed. */
function PasteField({ serverId }: { serverId: number }): ReactNode {
  const { t } = useTranslation("watchparty");
  const macos = useIsMacos();
  const [link, setLink] = useState("");
  const add = (text: string) => {
    const videoId = videoIdOfLink(text);
    if (videoId === null) {
      announce(t("playlist.notALink"), "assertive");
      return;
    }
    addToPlaylist(serverId, videoId, t("playlist.addFailed"));
    setLink("");
  };
  return (
    <label className="party-paste">
      <MediaIcon name="link" size={14} />
      <input
        type="text"
        className="party-paste__input"
        aria-label={t("playlist.paste")}
        placeholder={t("playlist.paste")}
        aria-keyshortcuts={macos ? "Meta+V" : "Control+V"}
        value={link}
        onPaste={(event) => {
          const text = event.clipboardData.getData("text");
          if (videoIdOfLink(text) === null) return;
          event.preventDefault();
          add(text);
        }}
        onChange={(event) => {
          setLink(event.target.value);
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter" && link !== "") {
            event.preventDefault();
            add(link);
          }
        }}
      />
      {link === "" && (
        // The shortcut is announced through aria-keyshortcuts; the chip is only a visual hint.
        <kbd className="party-paste__key" aria-hidden="true">
          {macos ? "⌘V" : t("pasteKey")}
        </kbd>
      )}
    </label>
  );
}

function Thumbnail({ videoId, badge }: { videoId: string; badge?: string }): ReactNode {
  const preview = useVideoPreview(videoId);
  const src = safeImage(preview?.image ?? null);
  return (
    <span className="party-thumb" aria-hidden="true">
      {src !== null && <img src={src} alt="" />}
      {badge !== undefined && <span className="party-thumb__badge">{badge}</span>}
    </span>
  );
}

/** Who added an entry: their avatar and name. */
function AddedBy({
  serverId,
  adder,
  name,
}: {
  serverId: number;
  adder: User | null;
  name: string;
}): ReactNode {
  const { t } = useTranslation("watchparty");
  const avatarOf = useAvatars(adder === null ? [] : [adder], serverId);
  return (
    <span className="party-entry__by">
      <Avatar name={name} src={adder === null ? null : avatarOf(adder)} size="sm" />
      <span aria-hidden="true">{name}</span>
      <span className="visually-hidden">{t("playlist.addedBy", { name })}</span>
    </span>
  );
}

function NowPlaying({
  serverId,
  videoId,
  adder,
  name,
  users,
}: {
  serverId: number;
  videoId: string;
  adder: Adder | null;
  name: string;
  users: ReadonlyMap<number, User>;
}): ReactNode {
  const { t } = useTranslation("watchparty");
  const preview = useVideoPreview(videoId);
  const durationMs = useKnownDuration(videoId);
  const title = preview?.title ?? t("playlist.videoTitle");
  return (
    <div className="party-now">
      <span className="party-now__mark">
        <MediaIcon name="play" size={10} />
      </span>
      <Thumbnail
        videoId={videoId}
        {...(durationMs === null ? {} : { badge: clockTime(durationMs / 1000) })}
      />
      <span className="party-entry__text">
        <span className="party-entry__title party-entry__title--one">{title}</span>
        {adder !== null && (
          <AddedBy serverId={serverId} adder={users.get(adder.session) ?? null} name={name} />
        )}
      </span>
    </div>
  );
}

function EntryRow({
  serverId,
  entry,
  name,
  adder,
  first,
  hintId,
  onMoveKey,
  onPlayNext,
}: {
  serverId: number;
  entry: PlaylistEntry;
  name: string;
  adder: User | null;
  first: boolean;
  hintId: string;
  onMoveKey: (event: KeyboardEvent<HTMLElement>) => void;
  onPlayNext: () => void;
}) {
  const { t } = useTranslation("watchparty");
  const preview = useVideoPreview(entry.videoId);
  const title = preview?.title ?? t("playlist.videoTitle");
  const [menu, setMenu] = useState<MenuPosition | null>(null);
  const more = t("playlist.more", { title });
  return (
    <>
      <button
        type="button"
        className="party-entry__handle"
        data-entry={entry.id}
        aria-label={t("playlist.move", { title })}
        aria-describedby={hintId}
        aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown"
        onKeyDown={onMoveKey}
      >
        <MediaIcon name="grip" size={14} />
      </button>
      <Thumbnail videoId={entry.videoId} />
      <span className="party-entry__text">
        <span className="party-entry__title" data-tooltip={title}>
          {title}
        </span>
        <AddedBy serverId={serverId} adder={adder} name={name} />
      </span>
      <button
        type="button"
        className="party-entry__more"
        aria-label={more}
        data-tooltip={more}
        aria-haspopup="menu"
        aria-expanded={menu !== null}
        onClick={(event) => {
          const box = event.currentTarget.getBoundingClientRect();
          setMenu({ x: box.right, y: box.bottom, opener: event.currentTarget });
        }}
      >
        <MediaIcon name="more" />
      </button>
      {menu !== null && (
        <ContextMenu
          menu={menu}
          label={more}
          items={[
            { label: t("playlist.playNext"), disabled: first, onSelect: onPlayNext },
            {
              label: t("playlist.removeShort"),
              danger: true,
              onSelect: () => {
                removeFromPlaylist(serverId, entry.id, t("playlist.removeFailed"));
              },
            },
          ]}
          onClose={() => {
            setMenu(null);
          }}
        />
      )}
    </>
  );
}
