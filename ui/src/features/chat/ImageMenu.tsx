/**
 * Context menu on a chat image (FR-1.52, QA.43, issue #115): copy or save the image, copy the
 * address of an external image, open it in the viewer, save it as a GIF favorite and open it in
 * the browser. Same look and keys as the other context menus. An external image that was not
 * loaded (link previews off, still loading or failed) only offers its address, so nothing is
 * fetched for it.
 */
import { useTranslation } from "react-i18next";
import { ContextMenu, type MenuItem, type MenuPosition } from "../../components/ContextMenu";
import { removeFavorite, saveFavorite, useGifFavorites } from "./favorites";
import { arrivedFullImage } from "./fullImage";
import { copyImage, imageActionError, saveImageAs } from "./imageActions";
import { openInBrowser } from "./openLink";

export interface ImageMenuState extends MenuPosition {
  /** What the chat shows; `null` when the image was not loaded. */
  src: string | null;
  /** Address of an external image. */
  url: string | null;
  /** Shown in the menu's label. */
  name: string;
  /** The image's own file name, `null` when unknown. */
  fileName: string | null;
  /** Reference of the full-size image, used instead of `src` once it has arrived. */
  full: string | null;
  /** Text selected when the menu opened; offered first (issue #114). */
  selection: string;
}

export interface ImageMenuProps {
  menu: ImageMenuState;
  onClose: () => void;
  /** Opens the image in the viewer; absent when there is nothing to show. */
  onOpenViewer?: (() => void) | undefined;
  /** Copies text and confirms it with `done`; `what` names it in the log. */
  copyText: (text: string, done: string, what: string) => void;
  /** Tells the user how an image action went. */
  notify: (text: string) => void;
}

export function ImageMenu({ menu, onClose, onOpenViewer, copyText, notify }: ImageMenuProps) {
  const { t } = useTranslation("chat");
  const favorites = useGifFavorites();
  const { src, url } = menu;
  const items: MenuItem[] = [];
  if (menu.selection !== "") {
    items.push({
      label: t("copy.selection"),
      onSelect: () => {
        copyText(menu.selection, t("copy.done"), "the selected text");
      },
    });
  }
  if (src !== null) {
    const original = (menu.full === null ? undefined : arrivedFullImage(menu.full)) ?? src;
    items.push(
      {
        label: t("imageMenu.copy"),
        onSelect: () => {
          copyImage(original).then(
            () => {
              notify(t("imageMenu.copied"));
            },
            (error: unknown) => {
              console.error("Copying an image failed", error);
              notify(imageActionError(error, t("imageMenu.copyFailed"), t("imageMenu.tooLarge")));
            },
          );
        },
      },
      {
        label: t("imageMenu.save"),
        onSelect: () => {
          saveImageAs(original, menu.fileName, t("viewer.save"), t("viewer.fileFilter")).then(
            (saved) => {
              if (saved) notify(t("viewer.saved"));
            },
            (error: unknown) => {
              console.error("Saving an image failed", error);
              notify(imageActionError(error, t("viewer.saveFailed"), t("imageMenu.tooLarge")));
            },
          );
        },
      },
    );
  }
  if (url !== null) {
    items.push({
      label: t("imageMenu.copyAddress"),
      onSelect: () => {
        copyText(url, t("imageMenu.addressCopied"), "an image address");
      },
    });
  }
  if (src !== null && onOpenViewer) {
    items.push({ label: t("imageMenu.openViewer"), onSelect: onOpenViewer });
  }
  if (src !== null && url !== null) {
    const saved = favorites.includes(url);
    items.push(
      {
        label: saved ? t("favorites.remove") : t("favorites.save"),
        onSelect: () => {
          if (saved) removeFavorite(url);
          else saveFavorite(url);
        },
      },
      {
        label: t("viewer.openInBrowser"),
        onSelect: () => {
          openInBrowser(url);
        },
      },
    );
  }
  return (
    <ContextMenu
      menu={menu}
      label={t("imageMenu.label", { name: menu.name })}
      onClose={onClose}
      items={items}
    />
  );
}
