/**
 * Cards under a message (FR-1.49, FR-1.50, FR-1.55, issues #260 and #262): a page card with site,
 * title, description and image, a player card for YouTube and Twitch or a music card whose player
 * loads only after a click, an audio player for direct audio links, or the image itself for
 * external images and image links. All text arrives as plain text and is
 * rendered as such; images are `data:` URLs from the engine, so the webview never contacts the
 * linked site.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { LinkPreview, VideoEmbed } from "../../api";
import { dismissPreview, useDismissedPreviews } from "./dismissedPreviews";
import {
  externalImages,
  isAudioLink,
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
import { parsePluginLink, pluginLinks, usePluginCardLinks } from "./pluginLink";
import { PluginLinkCard } from "./PluginLinkCard";
import { openInBrowser } from "./openLink";
import { canonicalVideoUrl } from "./videoLink";
import { ControlButton, DismissButton, OpenButton } from "./cardButtons";
import { EmbedHiddenHint } from "./EmbedHiddenHint";
import { hiddenEmbedKind } from "./embedHint";
import { InlineImageCard } from "./InlineImageCard";
import { AudioCard } from "./AudioCard";
import { MusicCard } from "./MusicCard";
import { useExclusivePlayback } from "./playback";
import { useFullscreen } from "../media/useFullscreen";
import { twitchNeedsBrowser } from "../watchparty";
import { appPageIsWebPage } from "./embedWindow";
import "../media/mediaOverlay.css";
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
  /** "Link previews in chat": without it only the `mumble://` card (without figures) and the
   * plugin card are shown, plus a hint naming what was hidden. */
  linkPreviews: boolean;
  /** Roving tab stop of the message the cards belong to. */
  tabIndex: number;
  /** Click on an inline image; without it the image opens in the browser. */
  onImageClick?: ImageClickHandler | undefined;
  /** The message's copy path, so "Link copied" shows in the chat's live region (issue #419). */
  onCopy: (text: string) => void;
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
  onCopy,
}: LinkPreviewsProps) {
  const links = useMemo(() => previewableLinks(html), [html]);
  const images = useMemo(() => externalImages(body), [body]);
  const mumble = useMemo(() => mumbleLinks(body), [body]);
  // Only plugin links that get a card take a preview slot.
  const plugins = usePluginCardLinks(useMemo(() => pluginLinks(body), [body]));
  const hidden = useDismissedPreviews(messageId, dismissed);
  // A `mumble://` link always gets its card, because the card is the message's own content; the
  // web previews need the setting (issue #175). A plugin link's card contacts no third party
  // either, so it is shown the same way (FR-6.65a).
  const own = [...mumble, ...plugins];
  const wanted = [...own, ...images, ...links.filter((url) => !images.includes(url))]
    .filter((url) => !hidden.has(url))
    .slice(0, MAX_PREVIEWS);
  const shown = linkPreviews ? wanted : wanted.filter((url) => own.includes(url));
  // What the switch hides gets one combined hint instead of vanishing silently (issue #261).
  const withheld = wanted
    .filter((url) => !shown.includes(url))
    .map((url) => hiddenEmbedKind(url, images));
  if (shown.length === 0 && withheld.length === 0) return null;
  const credentials = credentialLinks(mumble);
  return (
    <div className="link-previews">
      <EmbedHiddenHint kinds={withheld} tabIndex={tabIndex} />
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
        ) : plugins.includes(url) ? (
          <PluginLinkCard
            key={url}
            id={parsePluginLink(url) ?? ""}
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
        ) : isAudioLink(url) ? (
          <AudioCard
            key={url}
            url={url}
            tabIndex={tabIndex}
            onDismiss={() => {
              dismissPreview(messageId, url);
            }}
          />
        ) : (
          <LinkPreviewCard
            key={url}
            url={url}
            messageId={messageId}
            tabIndex={tabIndex}
            onImageClick={onImageClick}
            onCopy={onCopy}
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
  onCopy: (text: string) => void;
}

function LinkPreviewCard({ url, messageId, tabIndex, onImageClick, onCopy }: CardProps) {
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
  // The link turned out to serve audio (by content type or bytes): show the player.
  if (preview.isAudio) return <AudioCard url={url} tabIndex={tabIndex} onDismiss={dismiss} />;
  if (preview.music) {
    return (
      <MusicCard preview={preview} music={preview.music} tabIndex={tabIndex} onDismiss={dismiss} />
    );
  }
  return preview.video ? (
    <VideoCard
      preview={preview}
      video={preview.video}
      tabIndex={tabIndex}
      onDismiss={dismiss}
      onCopy={onCopy}
    />
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
};

interface VideoCardProps {
  preview: LinkPreview;
  video: VideoEmbed;
  tabIndex: number;
  onDismiss: () => void;
  onCopy: (text: string) => void;
}

function VideoCard({ preview, video, tabIndex, onDismiss, onCopy }: VideoCardProps) {
  const { t } = useTranslation("chat");
  const { t: tc } = useTranslation("common");
  const [playing, setPlaying] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const { fullscreen, toggle } = useFullscreen(box);
  const stop = useCallback(() => {
    setPlaying(false);
  }, []);
  useExclusivePlayback(playing, stop);
  const player = cardPlayer(video);
  // Twitch's player refuses to run under the app's own page unless it is a website (`tauri://` on
  // Linux and macOS), so its card stays a picture and Play hands the stream to the browser instead
  // of showing a box that stays black (#419).
  const inBrowser = twitchNeedsBrowser(video.playerUrl, appPageIsWebPage());
  const embedded = player !== null && !inBrowser;
  const site = preview.siteName ?? SITE_NAMES[video.site];
  const title = preview.title ?? t("preview.videoTitle", { site });
  const thumbnail = safeImage(preview.image);
  const copy = () => {
    onCopy(canonicalVideoUrl(preview.url));
  };
  const open = () => {
    openInBrowser(preview.url);
  };
  const start = () => {
    if (!embedded) open();
    else setPlaying(true);
  };
  // The controls are the watch party's media overlay (issue #163): they lie on the picture and fade
  // in on hover or keyboard focus, and the card counts as paused until it plays, so before the first
  // click the bar stays visible. The bar is inside the element that goes full screen, so a click can
  // leave it again (issue #419); Escape does the same.
  const controls = (
    <div className="media-overlay__bar">
      <div
        className="media-overlay__controls"
        role="toolbar"
        aria-label={tc("media.controls")}
        onKeyDown={(event) => {
          // F toggles full screen while the toolbar has the focus.
          if (event.key === "f" || event.key === "F") {
            event.preventDefault();
            toggle();
          }
        }}
      >
        {playing && embedded ? (
          <ControlButton
            label={fullscreen ? tc("media.exitFullscreen") : tc("media.fullscreen")}
            icon={fullscreen ? "exitFullscreen" : "fullscreen"}
            onClick={toggle}
            tabIndex={tabIndex}
            pressed={fullscreen}
            keyShortcuts="F"
          />
        ) : (
          <ControlButton
            label={t("preview.play", { title })}
            icon="play"
            main
            tooltip={embedded ? undefined : t("preview.inBrowser")}
            onClick={start}
            tabIndex={tabIndex}
          />
        )}
        <ControlButton label={t("linkMenu.copy")} icon="link" onClick={copy} tabIndex={tabIndex} />
        <ControlButton
          label={t("preview.openLabel", { title })}
          icon="open"
          onClick={open}
          tabIndex={tabIndex}
        />
      </div>
    </div>
  );
  return (
    <article
      className="link-card link-card--video"
      data-link-url={preview.url}
      aria-label={t("preview.card", { title })}
    >
      <div className="link-card__text">
        <p className="link-card__site">{site}</p>
        <p className="link-card__title">{title}</p>
        {video.channel !== null && <p className="link-card__description">{video.channel}</p>}
      </div>
      <div
        className="link-card__player-box media-overlay"
        ref={box}
        data-paused={!playing || undefined}
        data-fullscreen={fullscreen || undefined}
      >
        {playing && embedded ? (
          <PagePlayer
            playerUrl={player.url}
            title={t("preview.player", { title })}
            onFailed={() => {
              setPlaying(false);
              openInBrowser(preview.url);
            }}
          />
        ) : (
          thumbnail !== null && <img className="link-card__thumbnail" src={thumbnail} alt="" />
        )}
        {controls}
      </div>
      {inBrowser && <p className="link-card__browser-hint">{t("preview.inBrowserOnly")}</p>}
      {video.site === "youtube" && (
        <WatchTogetherButton playerUrl={video.playerUrl} title={title} tabIndex={tabIndex} />
      )}
      <DismissButton onDismiss={onDismiss} tabIndex={tabIndex} />
    </article>
  );
}
