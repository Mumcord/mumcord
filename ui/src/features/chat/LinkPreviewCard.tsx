/**
 * Preview cards under a message (FR-1.49, FR-1.50): a page card with site, title, description
 * and image, or a player card for YouTube and Twitch whose player loads only after a click.
 * All text arrives as plain text and is rendered as such; images are `data:` URLs from the
 * engine, so the webview never contacts the linked site.
 */
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { LinkPreview, VideoEmbed } from "../../api";
import { loadPreview, playerUrl, previewableLinks } from "./links";
import { openLink } from "./openLink";

function open(url: string) {
  openLink(url).catch((error: unknown) => {
    console.error("Opening a link failed", error);
  });
}

/** Only images the engine produced; anything else would make the webview load from the web. */
function safeImage(src: string | null): string | null {
  return src !== null && /^data:image\/(png|jpeg);base64,/.test(src) ? src : null;
}

interface LinkPreviewsProps {
  /** Sanitized message HTML. */
  html: string;
  /** Roving tab stop of the message the cards belong to. */
  tabIndex: number;
}

/** Cards for the links of one message; renders nothing until (and unless) a preview exists. */
export function LinkPreviews({ html, tabIndex }: LinkPreviewsProps) {
  const links = useMemo(() => previewableLinks(html), [html]);
  if (links.length === 0) return null;
  return (
    <div className="link-previews">
      {links.map((url) => (
        <LinkPreviewCard key={url} url={url} tabIndex={tabIndex} />
      ))}
    </div>
  );
}

function LinkPreviewCard({ url, tabIndex }: { url: string; tabIndex: number }) {
  const [preview, setPreview] = useState<LinkPreview | null>(null);
  useEffect(() => {
    let active = true;
    void loadPreview(url).then((loaded) => {
      if (active) setPreview(loaded);
    });
    return () => {
      active = false;
    };
  }, [url]);
  if (preview === null) return null;
  return preview.video ? (
    <VideoCard preview={preview} video={preview.video} tabIndex={tabIndex} />
  ) : (
    <PageCard preview={preview} tabIndex={tabIndex} />
  );
}

function OpenButton({ url, title, tabIndex }: { url: string; title: string; tabIndex: number }) {
  const { t } = useTranslation("chat");
  return (
    <button
      type="button"
      className="link-card__open"
      tabIndex={tabIndex}
      aria-label={t("preview.openLabel", { title })}
      onClick={() => {
        open(url);
      }}
    >
      {t("preview.open")}
    </button>
  );
}

function PageCard({ preview, tabIndex }: { preview: LinkPreview; tabIndex: number }) {
  const { t } = useTranslation("chat");
  const title = preview.title ?? preview.siteName ?? preview.url;
  const image = safeImage(preview.image);
  return (
    <article className="link-card" aria-label={t("preview.card", { title })}>
      <div className="link-card__text">
        {preview.siteName !== null && <p className="link-card__site">{preview.siteName}</p>}
        <p className="link-card__title">{title}</p>
        {preview.description !== null && (
          <p className="link-card__description">{preview.description}</p>
        )}
        <OpenButton url={preview.url} title={title} tabIndex={tabIndex} />
      </div>
      {image !== null && <img className="link-card__image" src={image} alt="" />}
    </article>
  );
}

interface VideoCardProps {
  preview: LinkPreview;
  video: VideoEmbed;
  tabIndex: number;
}

function VideoCard({ preview, video, tabIndex }: VideoCardProps) {
  const { t } = useTranslation("chat");
  const [playing, setPlaying] = useState(false);
  const player = playerUrl(video);
  const site = preview.siteName ?? (video.site === "youtube" ? "YouTube" : "Twitch");
  const title = preview.title ?? t("preview.videoTitle", { site });
  const thumbnail = safeImage(preview.image);
  return (
    <article className="link-card link-card--video" aria-label={t("preview.card", { title })}>
      <div className="link-card__text">
        <p className="link-card__site">{site}</p>
        <p className="link-card__title">{title}</p>
        {video.channel !== null && <p className="link-card__description">{video.channel}</p>}
      </div>
      {playing && player !== null ? (
        <iframe
          className="link-card__player"
          src={player}
          title={t("preview.player", { title })}
          allow="autoplay; encrypted-media; fullscreen; picture-in-picture"
          allowFullScreen
          referrerPolicy="strict-origin-when-cross-origin"
          sandbox="allow-scripts allow-same-origin allow-presentation"
        />
      ) : (
        <button
          type="button"
          className="link-card__play"
          tabIndex={tabIndex}
          aria-label={t("preview.play", { title })}
          title={player === null ? t("preview.inBrowser") : undefined}
          onClick={() => {
            if (player === null) open(preview.url);
            else setPlaying(true);
          }}
        >
          {thumbnail !== null && <img className="link-card__thumbnail" src={thumbnail} alt="" />}
          <span className="link-card__play-icon" aria-hidden="true">
            ▶
          </span>
        </button>
      )}
      <OpenButton url={preview.url} title={title} tabIndex={tabIndex} />
    </article>
  );
}
