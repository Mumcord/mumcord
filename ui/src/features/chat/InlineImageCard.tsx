/**
 * An external image or direct image link shown inline under a message (FR-1.55). The engine
 * fetches the image and delivers it as a `data:` URL, so the webview never contacts the
 * image's site. Images are scaled into `IMAGE_BOX` (400×300) with their aspect ratio kept; a box
 * of that size holds the place while the image loads.
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { InlineImage } from "../../api";
import { DismissButton, OpenButton } from "./cardButtons";
import { dismissPreview } from "./dismissedPreviews";
import { useDisplaySrc } from "./displaySrc";
import { GifAttribution } from "./GifAttribution";
import { removeFavorite, saveFavorite, useGifFavorites } from "./favorites";
import { markInlineShown } from "./inlineLinks";
import { fitImage, IMAGE_BOX, imageName, loadInlineImage, type ImageClickHandler } from "./links";
import { openInBrowser } from "./openLink";

/** Only images the engine produced; anything else would make the webview load from the web. */
const SAFE_SRC = /^data:image\/(?:png|jpeg|gif|webp);base64,/;

interface InlineImageCardProps {
  url: string;
  /** The message the image belongs to; dismissals are remembered per message. */
  messageId: string;
  /** Roving tab stop of the message. */
  tabIndex: number;
  /** Click on the image; without it the image opens in the browser. */
  onImageClick?: ImageClickHandler | undefined;
}

export function InlineImageCard({ url, messageId, tabIndex, onImageClick }: InlineImageCardProps) {
  const { t } = useTranslation("chat");
  // `undefined` while loading, `null` when there is nothing to show.
  const [image, setImage] = useState<InlineImage | null | undefined>(undefined);
  useEffect(() => {
    let active = true;
    void loadInlineImage(url).then((loaded) => {
      if (active) setImage(loaded);
    });
    return () => {
      active = false;
    };
  }, [url]);
  const visible = image !== undefined && image !== null && SAFE_SRC.test(image.src);
  const src = useDisplaySrc(visible ? image.src : undefined);
  useEffect(() => {
    if (!visible) return;
    return markInlineShown(messageId, url);
  }, [visible, messageId, url]);
  const favorites = useGifFavorites();
  const name = imageName(url);
  if (image === undefined) {
    return (
      <div
        className="inline-image inline-image--loading"
        role="img"
        aria-busy="true"
        aria-label={t("image.loading", { name })}
        data-image-url={url}
        style={{ width: IMAGE_BOX.width, height: IMAGE_BOX.height }}
      />
    );
  }
  if (image === null || !SAFE_SRC.test(image.src)) {
    // Too large, an unsupported type or unreachable: say so and keep the link usable.
    return (
      <div
        className="inline-image inline-image--failed"
        role="group"
        aria-label={t("image.label", { name })}
        data-image-url={url}
      >
        <span className="inline-image__failed-text">{t("image.failed")}</span>
        <OpenButton
          url={url}
          title={name}
          tabIndex={tabIndex}
          className="inline-image__failed-open"
        />
        <DismissButton
          tabIndex={tabIndex}
          onDismiss={() => {
            dismissPreview(messageId, url);
          }}
        />
      </div>
    );
  }
  const size = fitImage(image.width, image.height);
  const favorite = favorites.includes(image.url);
  const favoriteLabel = favorite ? t("favorites.remove") : t("favorites.save");

  return (
    // The message list opens the image menu for a right-click or the menu key in here.
    <figure
      className="inline-image"
      aria-label={t("image.label", { name })}
      style={{ width: size.width }}
    >
      <button
        type="button"
        className="inline-image__view"
        tabIndex={tabIndex}
        aria-label={t("image.view", { name })}
        data-tooltip={image.url}
        onClick={(event) => {
          if (onImageClick) onImageClick(image, event.currentTarget);
          else openInBrowser(image.url);
        }}
      >
        <img
          className="inline-image__img"
          src={src}
          alt=""
          draggable={false}
          data-image-url={image.url}
          width={size.width}
          height={size.height}
        />
      </button>
      <GifAttribution url={image.url} className="inline-image__attribution" tabIndex={tabIndex} />
      <OpenButton url={image.url} title={name} tabIndex={tabIndex} className="inline-image__open" />
      <button
        type="button"
        className="inline-image__favorite"
        tabIndex={tabIndex}
        aria-label={favoriteLabel}
        data-tooltip={favoriteLabel}
        onClick={() => {
          if (favorite) removeFavorite(image.url);
          else saveFavorite(image.url);
        }}
      >
        <span aria-hidden="true">{favorite ? "★" : "☆"}</span>
      </button>
      <DismissButton
        tabIndex={tabIndex}
        onDismiss={() => {
          dismissPreview(messageId, url);
        }}
      />
    </figure>
  );
}
