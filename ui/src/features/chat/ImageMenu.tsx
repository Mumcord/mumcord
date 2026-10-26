/**
 * Context menu on an inline image: save it as a GIF favorite (or remove it) and open it in the
 * browser (FR-1.52, QA.43). Same look and keys as the other context menus.
 */
import { useTranslation } from "react-i18next";
import { ContextMenu, type MenuPosition } from "../../components/ContextMenu";
import { removeFavorite, saveFavorite, useGifFavorites } from "./favorites";
import { openInBrowser } from "./openLink";

export interface ImageMenuState extends MenuPosition {
  url: string;
  name: string;
}

export function ImageMenu({ menu, onClose }: { menu: ImageMenuState; onClose: () => void }) {
  const { t } = useTranslation("chat");
  const favorites = useGifFavorites();
  const saved = favorites.includes(menu.url);
  return (
    <ContextMenu
      menu={menu}
      label={t("imageMenu.label", { name: menu.name })}
      onClose={onClose}
      items={[
        saved
          ? {
              label: t("favorites.remove"),
              onSelect: () => {
                removeFavorite(menu.url);
              },
            }
          : {
              label: t("favorites.save"),
              onSelect: () => {
                saveFavorite(menu.url);
              },
            },
        {
          label: t("viewer.openInBrowser"),
          onSelect: () => {
            openInBrowser(menu.url);
          },
        },
      ]}
    />
  );
}
