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
import { fitImage, IMAGE_BOX, loadInlineImage, type ImageClickHandler } from "./links";
import { openInBrowser } from "./openLink";

/** Only images the engine produced; anything else would make the webview load from the web. */
const SAFE_SRC = /^data:image\/(?:png|jpeg|gif|webp);base64,/;

/** File name of the image link for labels, or its host when the path has none. */
function imageName(url: string): string {
  try {
    const parsed = new URL(url);
    const last = parsed.pathname.split("/").pop() ?? "";
    try {
      return decodeURIComponent(last) || parsed.hostname;
    } catch {
      return last || parsed.hostname;
    }
  } catch {
    return url;
  }
}

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
  const name = imageName(url);
  if (image === undefined) {
    return (
      <div
        className="inline-image inline-image--loading"
        role="img"
        aria-busy="true"
        aria-label={t("image.loading", { name })}
        style={{ width: IMAGE_BOX.width, height: IMAGE_BOX.height }}
      />
    );
  }
  if (image === null || !SAFE_SRC.test(image.src)) return null;
  const size = fitImage(image.width, image.height);
  return (
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
        onClick={() => {
          if (onImageClick) onImageClick(image);
          else openInBrowser(image.url);
        }}
      >
        <img
          className="inline-image__img"
          src={image.src}
          alt=""
          width={size.width}
          height={size.height}
        />
      </button>
      <OpenButton url={image.url} title={name} tabIndex={tabIndex} className="inline-image__open" />
      <DismissButton
        tabIndex={tabIndex}
        onDismiss={() => {
          dismissPreview(messageId, url);
        }}
      />
    </figure>
  );
}
