/**
 * GIF button of the message input and its favorites picker (FR-1.52, QA.43): a grid of the
 * saved GIFs; choosing one sends its link, so other clients see a link and Mumcord shows it
 * inline. Thumbnails come through the engine like inline images; with link previews switched
 * off nothing is loaded and the grid shows file names instead.
 */
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { useHintPreferences } from "../../a11y/hintPreferences";
import type { InlineImage } from "../../api";
import { GifAttribution } from "./GifAttribution";
import { markFavoriteUsed, removeFavorite, useGifFavorites } from "./favorites";
import { imageName, loadInlineImage } from "./links";

const COLUMNS = 4;
const GIF_ICON = "M3 6h18v12H3zM8 10.5H6v3h2v-1.5M11 10v4M14 14v-4h2.5M14 12h2";

/** Thumbnails are only ever the engine's `data:` images. */
const SAFE_SRC = /^data:image\/(?:png|jpeg|gif|webp);base64,/;

function Thumbnail({ url, linkPreviews }: { url: string; linkPreviews: boolean }) {
  const [image, setImage] = useState<InlineImage | null>(null);
  const [visible, setVisible] = useState(typeof IntersectionObserver === "undefined");
  const holder = useRef<HTMLSpanElement>(null);
  // Only thumbnails in view are loaded and animated; scrolled-out ones are released again, so
  // a long favorites list never decodes dozens of animated GIFs at once.
  useEffect(() => {
    const element = holder.current;
    if (!element || typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver((entries) => {
      const last = entries[entries.length - 1];
      if (last) setVisible(last.isIntersecting);
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, []);
  useEffect(() => {
    if (!linkPreviews || !visible) return;
    let active = true;
    void loadInlineImage(url).then((loaded) => {
      if (active) setImage(loaded);
    });
    return () => {
      active = false;
    };
  }, [url, linkPreviews, visible]);
  // The wrapper stays mounted so the observer keeps watching after the image has loaded.
  return (
    <span ref={holder} className="gif-picker__thumb">
      {linkPreviews && visible && image && SAFE_SRC.test(image.src) ? (
        <img className="gif-picker__img" src={image.src} alt="" draggable={false} />
      ) : (
        <span className="gif-picker__placeholder" aria-hidden="true">
          {imageName(url)}
        </span>
      )}
    </span>
  );
}

export interface GifPickerProps {
  /** Sends the link as a message. */
  onSend: (url: string) => Promise<void>;
  disabled: boolean;
  linkPreviews: boolean;
}

export function GifPicker({ onSend, disabled, linkPreviews }: GifPickerProps) {
  const { t } = useTranslation("chat");
  const favorites = useGifFavorites();
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const root = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const dialog = useRef<HTMLDivElement>(null);
  const cells = useRef<(HTMLButtonElement | null)[]>([]);
  const dialogId = useId();
  const hintId = useId();
  const { keyboardHints } = useHintPreferences();
  const current = Math.min(active, Math.max(favorites.length - 1, 0));

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!(event.target instanceof Node && root.current?.contains(event.target))) setOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
    };
  }, [open]);

  // Focus moves into the popover when it opens: the first GIF, or the explanation if empty.
  useEffect(() => {
    if (!open) return;
    (cells.current[0] ?? dialog.current)?.focus();
    // Only when opening; later list changes keep the focus where it is.
  }, [open]);

  // Favorites still loading when the popover opened: move on from the placeholder focus.
  const hasFavorites = favorites.length > 0;
  useEffect(() => {
    if (open && hasFavorites && document.activeElement === dialog.current)
      cells.current[0]?.focus();
  }, [open, hasFavorites]);

  function close() {
    setOpen(false);
    button.current?.focus();
  }

  function focusCell(index: number) {
    const next = Math.max(0, Math.min(index, favorites.length - 1));
    setActive(next);
    cells.current[next]?.focus();
  }

  function remove(index: number) {
    const url = favorites[index];
    if (url === undefined) return;
    removeFavorite(url);
    const left = favorites.length - 1;
    if (left === 0) dialog.current?.focus();
    else focusCell(Math.min(index, left - 1));
  }

  function choose(url: string) {
    close();
    void onSend(url).then(
      () => {
        markFavoriteUsed(url);
      },
      () => undefined,
    );
  }

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close();
      return;
    }
    if (event.key === "Tab") {
      setOpen(false);
      return;
    }
    if (favorites.length === 0) return;
    const last = favorites.length - 1;
    switch (event.key) {
      case "ArrowRight":
        focusCell(current + 1);
        break;
      case "ArrowLeft":
        focusCell(current - 1);
        break;
      case "ArrowDown":
        focusCell(Math.min(current + COLUMNS, last));
        break;
      case "ArrowUp":
        focusCell(current - COLUMNS);
        break;
      case "Home":
        focusCell(0);
        break;
      case "End":
        focusCell(last);
        break;
      case "Delete":
      case "Backspace":
        remove(current);
        break;
      default:
        return;
    }
    event.preventDefault();
  }

  return (
    <div ref={root} className="gif-picker">
      <button
        ref={button}
        type="button"
        className="chat-composer__icon chat-composer__gif"
        aria-label={t("favorites.open")}
        data-tooltip={t("favorites.open")}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-controls={open ? dialogId : undefined}
        disabled={disabled}
        onClick={() => {
          setActive(0);
          setOpen(!open);
        }}
      >
        <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" focusable="false">
          <path
            d={GIF_ICON}
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      </button>
      {open ? (
        // Keys are handled on the dialog; every control inside is a real button.
        // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions
        <div
          ref={dialog}
          id={dialogId}
          className="gif-picker__popover"
          role="dialog"
          aria-label={t("favorites.title")}
          aria-describedby={hintId}
          tabIndex={-1}
          onKeyDown={onKeyDown}
        >
          {favorites.length === 0 ? (
            <p id={hintId} className="gif-picker__empty">
              {t("favorites.empty")}
            </p>
          ) : (
            <>
              <p id={hintId} className={keyboardHints ? "gif-picker__hint" : "visually-hidden"}>
                {t("favorites.hint")}
              </p>
              <ul className="gif-picker__grid" aria-label={t("favorites.title")}>
                {favorites.map((url, index) => {
                  const name = imageName(url);
                  return (
                    <li key={url} className="gif-picker__cell">
                      <button
                        ref={(element) => {
                          cells.current[index] = element;
                        }}
                        type="button"
                        className="gif-picker__choose"
                        tabIndex={index === current ? 0 : -1}
                        aria-label={t("favorites.send", { name })}
                        data-tooltip={linkPreviews ? name : url}
                        onClick={() => {
                          choose(url);
                        }}
                      >
                        <Thumbnail url={url} linkPreviews={linkPreviews} />
                      </button>
                      <GifAttribution url={url} className="gif-picker__attribution" tabIndex={-1} />
                      <button
                        type="button"
                        className="gif-picker__remove"
                        tabIndex={-1}
                        aria-label={t("favorites.removeNamed", { name })}
                        data-tooltip={t("favorites.remove")}
                        onClick={() => {
                          remove(index);
                        }}
                      >
                        <span aria-hidden="true">×</span>
                      </button>
                    </li>
                  );
                })}
              </ul>
            </>
          )}
        </div>
      ) : null}
    </div>
  );
}
