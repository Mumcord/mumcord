/**
 * The shared playlist of the watch party (FR-2.69): what plays next, with thumbnail, title and
 * who added it. Everyone in the party can remove entries, drag them into another order or move
 * the focused entry with Alt+Up / Alt+Down, and skip to the next video. Title and thumbnail come
 * from the engine's link preview of the video (the same one a chat card shows), so no other
 * service is asked and nothing from the other clients is rendered but the video id.
 */
import {
  useEffect,
  useId,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import type { LinkPreview, PlaylistEntry, User, WatchParty } from "../../api";
import { loadPreview, safeImage } from "../chat/links";
import { Avatar } from "../members/Avatar";
import { useAvatars } from "../members/useAvatars";
import { WatchLinkDialog } from "./WatchLinkDialog";
import { moveInPlaylist, playNext, removeFromPlaylist } from "./playlistActions";
import { videoIdOfLink, watchUrl } from "./youtube";
import { addToPlaylist } from "./playlistActions";

interface PlaylistProps {
  serverId: number;
  party: WatchParty;
  users: ReadonlyMap<number, User>;
  /** Controls before the skip button in the slim control row, e.g. play/pause. */
  leading?: ReactNode;
  /** Shows only "Up next" with the list in a popover; otherwise the list is always shown. */
  compact?: boolean;
  /** The "Paste a YouTube link" field above the list (issue #163). */
  paste?: boolean;
}

const POPOVER_MIN_WIDTH = 288;
/** Free space above the line that decides whether the popover opens upwards. */
const POPOVER_ROOM = 200;
const POPOVER_CLOSE_MS = 150;

export function Playlist({
  serverId,
  party,
  users,
  leading,
  compact = false,
  paste = false,
}: PlaylistProps) {
  const { t } = useTranslation("watchparty");
  const entries = party.playlist;
  const hintId = useId();
  const upNextId = useId();
  const [dragged, setDragged] = useState<number | null>(null);
  // The entry to keep focused after the list re-renders in its new order.
  const [focusEntry, setFocusEntry] = useState<number | null>(null);
  const list = useRef<HTMLOListElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const [popover, setPopover] = useState<CSSProperties | null>(null);
  // While the add dialog is open the popover stays, so the dialog is not unmounted under the user.
  const [adding, setAdding] = useState(false);
  const [link, setLink] = useState("");
  const addingRef = useRef(false);
  const closeTimer = useRef<number | undefined>(undefined);
  useEffect(() => {
    addingRef.current = adding;
  }, [adding]);
  const next = usePreview(entries[0]?.videoId ?? null);

  const add = (text: string) => {
    const videoId = videoIdOfLink(text);
    if (videoId === null) {
      announce(t("playlist.notALink"), "assertive");
      return;
    }
    addToPlaylist(serverId, videoId, t("playlist.addFailed"));
    setLink("");
  };

  useEffect(() => {
    if (focusEntry === null) return;
    list.current?.querySelector<HTMLElement>(`[data-entry="${String(focusEntry)}"]`)?.focus();
  }, [focusEntry, entries]);

  const move = (entry: PlaylistEntry, index: number) => {
    if (index < 0 || index >= entries.length) return;
    moveInPlaylist(serverId, entry.id, index, t("playlist.moveFailed"));
  };

  const onKeyDown = (event: KeyboardEvent<HTMLElement>, entry: PlaylistEntry, index: number) => {
    if (!event.altKey || (event.key !== "ArrowUp" && event.key !== "ArrowDown")) return;
    event.preventDefault();
    const target = event.key === "ArrowUp" ? index - 1 : index + 1;
    if (target < 0 || target >= entries.length) return;
    setFocusEntry(entry.id);
    move(entry, target);
    announce(t("playlist.moved", { position: target + 1 }), "polite");
  };

  const skip = () => {
    if (entries.length === 0) return;
    playNext(serverId, party.currentEntry, true, t("playlist.skipFailed"));
  };

  const listBody = (
    <>
      {entries.length === 0 ? (
        <p className="watch-playlist__empty">{t("playlist.empty")}</p>
      ) : (
        <>
          <p id={hintId} className="visually-hidden">
            {t("playlist.moveHint")}
          </p>
          <ol ref={list} className="watch-playlist__list">
            {entries.map((entry, index) => (
              // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- dragging is the pointer way to reorder; the handle button moves entries by keyboard
              <li
                key={entry.id}
                className={`watch-playlist__entry${dragged === entry.id ? " watch-playlist__entry--dragged" : ""}`}
                draggable
                onDragStart={(event) => {
                  event.dataTransfer.effectAllowed = "move";
                  event.dataTransfer.setData("text/plain", String(entry.id));
                  setDragged(entry.id);
                }}
                onDragEnd={() => {
                  setDragged(null);
                }}
                onDragOver={(event) => {
                  if (dragged !== null) event.preventDefault();
                }}
                onDrop={(event) => {
                  event.preventDefault();
                  const from = entries.find((e) => e.id === dragged);
                  setDragged(null);
                  if (from !== undefined && from.id !== entry.id) move(from, index);
                }}
              >
                <EntryRow
                  serverId={serverId}
                  entry={entry}
                  users={users}
                  hintId={hintId}
                  onMoveKey={(event) => {
                    onKeyDown(event, entry, index);
                  }}
                />
              </li>
            ))}
          </ol>
        </>
      )}
    </>
  );

  // Fixed, not absolute: the widget clips its content, the popover must reach out of it.
  const show = () => {
    window.clearTimeout(closeTimer.current);
    const box = trigger.current?.getBoundingClientRect();
    if (box === undefined) return;
    const width = Math.min(Math.max(box.width, POPOVER_MIN_WIDTH), window.innerWidth - 16);
    const left = Math.min(Math.max(box.left, 8), window.innerWidth - width - 8);
    setPopover(
      box.top > POPOVER_ROOM
        ? { left, width, bottom: window.innerHeight - box.top + 2 }
        : { left, width, top: box.bottom + 2 },
    );
  };
  // A short delay lets the pointer cross the gap between the line and the popover.
  const hideSoon = () => {
    window.clearTimeout(closeTimer.current);
    closeTimer.current = window.setTimeout(() => {
      if (!addingRef.current) setPopover(null);
    }, POPOVER_CLOSE_MS);
  };
  useEffect(
    () => () => {
      window.clearTimeout(closeTimer.current);
    },
    [],
  );

  const skipLabel = t("playlist.skip");
  const upNext =
    entries.length === 0
      ? t("playlist.upNextNone")
      : t("playlist.upNext", { title: next?.title ?? t("playlist.videoTitle") });
  return (
    <section className="watch-playlist" aria-label={t("playlist.title")}>
      {paste && (
        <p className="watch-playlist__paste">
          <input
            type="text"
            aria-label={t("playlist.paste")}
            placeholder={t("playlist.paste")}
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
        </p>
      )}
      <div className="watch-playlist__header">
        {leading}
        <button
          type="button"
          className="button watch-party__icon"
          data-tooltip={skipLabel}
          aria-label={skipLabel}
          aria-disabled={entries.length === 0 || undefined}
          aria-describedby={entries.length === 0 ? upNextId : undefined}
          onClick={skip}
        >
          <span aria-hidden="true">⏭</span>
        </button>
        {compact ? (
          // eslint-disable-next-line jsx-a11y/no-static-element-interactions -- groups the focusable "Up next" button and its popover; hover and focus only open it, Escape closes it
          <div
            className="watch-playlist__next"
            onMouseEnter={show}
            onMouseLeave={hideSoon}
            onFocus={show}
            onBlur={(event) => {
              if (!event.currentTarget.contains(event.relatedTarget)) hideSoon();
            }}
            onKeyDown={(event) => {
              if (event.key === "Escape" && popover !== null) {
                event.stopPropagation();
                setPopover(null);
              }
            }}
          >
            <button
              ref={trigger}
              type="button"
              id={upNextId}
              className="watch-playlist__up-next"
              data-tooltip={upNext}
              aria-haspopup="true"
              aria-expanded={popover !== null}
              onClick={show}
            >
              {upNext}
            </button>
            {popover !== null && (
              <div className="watch-playlist__popover" style={popover}>
                {listBody}
                <button
                  type="button"
                  className="button watch-playlist__add"
                  onClick={() => {
                    setAdding(true);
                  }}
                >
                  <span aria-hidden="true">+</span> {t("addVideo")}
                </button>
              </div>
            )}
          </div>
        ) : (
          <p id={upNextId} className="watch-playlist__up-next" data-tooltip={upNext}>
            {upNext}
          </p>
        )}
      </div>
      {!compact && listBody}
      {adding && (
        <WatchLinkDialog
          serverId={serverId}
          onClose={() => {
            setAdding(false);
          }}
        />
      )}
    </section>
  );
}

function usePreview(videoId: string | null): LinkPreview | null {
  const [preview, setPreview] = useState<{ videoId: string; preview: LinkPreview | null }>();
  useEffect(() => {
    if (videoId === null) return;
    let current = true;
    void loadPreview(watchUrl(videoId)).then((loaded) => {
      if (current) setPreview({ videoId, preview: loaded });
    });
    return () => {
      current = false;
    };
  }, [videoId]);
  return preview?.videoId === videoId ? preview.preview : null;
}

function EntryRow({
  serverId,
  entry,
  users,
  hintId,
  onMoveKey,
}: {
  serverId: number;
  entry: PlaylistEntry;
  users: ReadonlyMap<number, User>;
  hintId: string;
  onMoveKey: (event: KeyboardEvent<HTMLElement>) => void;
}) {
  const { t } = useTranslation("watchparty");
  const preview = usePreview(entry.videoId);
  const title = preview?.title ?? t("playlist.videoTitle");
  const thumbnail = safeImage(preview?.image ?? null);
  const adder = users.get(entry.addedBy);
  const avatarOf = useAvatars(adder ? [adder] : [], serverId);
  const name = adder?.name ?? entry.addedByName ?? t("someone");
  return (
    <>
      <button
        type="button"
        className="watch-playlist__handle"
        data-entry={entry.id}
        aria-label={t("playlist.move", { title })}
        aria-describedby={hintId}
        onKeyDown={onMoveKey}
      >
        ⠿
      </button>
      {thumbnail !== null ? (
        <img className="watch-playlist__thumbnail" src={thumbnail} alt="" />
      ) : (
        <span className="watch-playlist__thumbnail" aria-hidden="true" />
      )}
      <span className="watch-playlist__text">
        <span className="watch-playlist__name" data-tooltip={title}>
          {title}
        </span>
      </span>
      <span
        className="watch-playlist__by"
        role="img"
        data-tooltip={t("playlist.addedBy", { name })}
        aria-label={t("playlist.addedBy", { name })}
      >
        <Avatar name={name} src={adder ? avatarOf(adder) : null} size="sm" />
      </span>
      <button
        type="button"
        className="button watch-playlist__remove"
        aria-label={t("playlist.remove", { title })}
        data-tooltip={t("playlist.remove", { title })}
        onClick={() => {
          removeFromPlaylist(serverId, entry.id, t("playlist.removeFailed"));
        }}
      >
        ✕
      </button>
    </>
  );
}
