/**
 * Cards under a message (FR-1.49, FR-1.50, FR-1.55): a page card with site, title, description
 * and image, a player card for YouTube, Twitch and Spotify whose player loads only after a click, or
 * the image itself for external images and image links. All text arrives as plain text and is
 * rendered as such; images are `data:` URLs from the engine, so the webview never contacts the
 * linked site.
 */
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { LinkPreview, VideoEmbed } from "../../api";
import { dismissPreview, useDismissedPreviews } from "./dismissedPreviews";
import {
  externalImages,
  isImageLink,
  cardPlayer,
  loadPreview,
  MAX_PREVIEWS,
  previewableLinks,
  safeImage,
  type ImageClickHandler,
} from "./links";
import { credentialLinks, mumbleLinks } from "./mumbleLink";

import { MumbleLinkCard } from "./MumbleLinkCard";
import { openInBrowser } from "./openLink";
import { DismissButton, OpenButton } from "./cardButtons";
import { InlineImageCard } from "./InlineImageCard";
import { PagePlayer } from "./PagePlayer";
import { WatchTogetherButton } from "../watchparty";

interface LinkPreviewsProps {
  /** The message the cards belong to; dismissals are remembered per message. */
  messageId: string;
  /** Links of the message whose cards were dismissed earlier. */
  dismissed: readonly string[];
  /** Sanitized message HTML. */
  html: string;
  /** Message HTML as the engine delivered it; still holds external `<img src>` URLs. */
  body: string;
  /** "Link previews in chat": without it only the `mumble://` card is shown, without figures. */
  linkPreviews: boolean;
  /** Roving tab stop of the message the cards belong to. */
  tabIndex: number;
  /** Click on an inline image; without it the image opens in the browser. */
  onImageClick?: ImageClickHandler | undefined;
}

/**
 * Cards for the external images and links of one message; renders nothing until (and unless)
 * a preview or image exists.
 */
export function LinkPreviews({
  messageId,
  dismissed,
  html,
  body,
  linkPreviews,
  tabIndex,
  onImageClick,
}: LinkPreviewsProps) {
  const links = useMemo(() => previewableLinks(html), [html]);
  const images = useMemo(() => externalImages(body), [body]);
  const mumble = useMemo(() => mumbleLinks(body), [body]);
  const hidden = useDismissedPreviews(messageId, dismissed);
  // A `mumble://` link always gets its card, because the card is the message's own content; the
  // web previews need the setting (issue #175).
  const shown = [
    ...mumble,
    ...(linkPreviews ? [...images, ...links.filter((url) => !images.includes(url))] : []),
  ]
    .filter((url) => !hidden.has(url))
    .slice(0, MAX_PREVIEWS);
  if (shown.length === 0) return null;
  const credentials = credentialLinks(mumble);
  return (
    <div className="link-previews">
      {shown.map((url) =>
        mumble.includes(url) ? (
          <MumbleLinkCard
            key={url}
            url={url}
            credentialLinks={credentials}
            tabIndex={tabIndex}
            onDismiss={() => {
              dismissPreview(messageId, url);
            }}
          />
        ) : images.includes(url) || isImageLink(url) ? (
          <InlineImageCard
            key={url}
            url={url}
            messageId={messageId}
            tabIndex={tabIndex}
            onImageClick={onImageClick}
          />
        ) : (
          <LinkPreviewCard
            key={url}
            url={url}
            messageId={messageId}
            tabIndex={tabIndex}
            onImageClick={onImageClick}
          />
        ),
      )}
    </div>
  );
}

interface CardProps {
  url: string;
  messageId: string;
  tabIndex: number;
  onImageClick: ImageClickHandler | undefined;
}

function LinkPreviewCard({ url, messageId, tabIndex, onImageClick }: CardProps) {
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
  // The link turned out to serve an image (by content type): show the image itself.
  if (preview.isImage)
    return (
      <InlineImageCard
        url={url}
        messageId={messageId}
        tabIndex={tabIndex}
        onImageClick={onImageClick}
      />
    );
  const dismiss = () => {
    dismissPreview(messageId, url);
  };
  return preview.video ? (
    <VideoCard preview={preview} video={preview.video} tabIndex={tabIndex} onDismiss={dismiss} />
  ) : (
    <PageCard preview={preview} tabIndex={tabIndex} onDismiss={dismiss} />
  );
}

interface PageCardProps {
  preview: LinkPreview;
  tabIndex: number;
  onDismiss: () => void;
}

function PageCard({ preview, tabIndex, onDismiss }: PageCardProps) {
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
      <DismissButton onDismiss={onDismiss} tabIndex={tabIndex} />
    </article>
  );
}

const SITE_NAMES: Record<VideoEmbed["site"], string> = {
  youtube: "YouTube",
  twitch: "Twitch",
  spotify: "Spotify",
};

const SPOTIFY_KINDS = ["track", "album", "playlist", "artist", "episode", "show"] as const;

function spotifyKind(kind: string): (typeof SPOTIFY_KINDS)[number] | null {
  return SPOTIFY_KINDS.find((k) => k === kind) ?? null;
}

interface VideoCardProps {
  preview: LinkPreview;
  video: VideoEmbed;
  tabIndex: number;
  onDismiss: () => void;
}

function VideoCard({ preview, video, tabIndex, onDismiss }: VideoCardProps) {
  const { t } = useTranslation("chat");
  const [playing, setPlaying] = useState(false);
  const player = cardPlayer(video);
  const spotify = video.site === "spotify";
  const site = preview.siteName ?? SITE_NAMES[video.site];
  const title =
    preview.title ?? t(spotify ? "preview.spotifyTitle" : "preview.videoTitle", { site });
  // Spotify: the channel field holds the kind of item, e.g. "track".
  const kind = spotify && video.channel !== null ? spotifyKind(video.channel) : null;
  const thumbnail = safeImage(preview.image);
  return (
    <article className="link-card link-card--video" aria-label={t("preview.card", { title })}>
      <div className="link-card__text">
        <p className="link-card__site">{site}</p>
        <p className="link-card__title">{title}</p>
        {kind !== null && <p className="link-card__description">{t(`preview.kind.${kind}`)}</p>}
        {spotify && preview.description !== null && (
          <p className="link-card__description">{preview.description}</p>
        )}
        {!spotify && video.channel !== null && (
          <p className="link-card__description">{video.channel}</p>
        )}
      </div>
      {playing && player?.via === "page" ? (
        <PagePlayer
          playerUrl={player.url}
          title={t("preview.player", { title })}
          onFailed={() => {
            setPlaying(false);
            openInBrowser(preview.url);
          }}
        />
      ) : playing && player !== null ? (
        <iframe
          className={`link-card__player${spotify ? " link-card__player--audio" : ""}`}
          src={player.url}
          title={t(spotify ? "preview.audioPlayer" : "preview.player", { title })}
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
          data-tooltip={player === null ? t("preview.inBrowser") : undefined}
          onClick={() => {
            if (player === null) openInBrowser(preview.url);
            else setPlaying(true);
          }}
        >
          {thumbnail !== null && <img className="link-card__thumbnail" src={thumbnail} alt="" />}
          <span className="link-card__play-icon" aria-hidden="true">
            ▶
          </span>
        </button>
      )}
      {video.site === "youtube" && (
        <WatchTogetherButton playerUrl={video.playerUrl} title={title} tabIndex={tabIndex} />
      )}
      <OpenButton url={preview.url} title={title} tabIndex={tabIndex} />
      <DismissButton onDismiss={onDismiss} tabIndex={tabIndex} />
    </article>
  );
}
