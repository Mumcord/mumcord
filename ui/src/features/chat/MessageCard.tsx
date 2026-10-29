/**
 * A message card (issue #72, docs/message-cards.md): status stripe, icon, title, the text with
 * chat formatting, an optional image and link. The image follows the link preview rules: only
 * with previews on, fetched by the engine and shown as a `data:` URL; with previews off it stays
 * a plain link and nothing is requested. Links are ordinary anchors, so the message list's click
 * handler and link menu apply to them as to chat links.
 */
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { InlineImage } from "../../api";
import { displayUrl, type CardColor, type CardIcon, type MessageCard as Card } from "./cards";
import { useCodeBlockActions } from "./useCodeBlockActions";
import { highlightCode } from "./highlight";
import { useDisplaySrc } from "./displaySrc";
import { imageName, loadInlineImage, type ImageClickHandler } from "./links";
import { openInBrowser } from "./openLink";

/** Only images the engine produced; anything else would make the webview load from the web. */
const SAFE_SRC = /^data:image\/(?:png|jpeg|gif|webp);base64,/;

const STATUS_SYMBOL: Record<CardColor, string> = {
  info: "ⓘ",
  success: "✓",
  warning: "▲",
  error: "✕",
};

/** 24×24 line icons, drawn with the current colour. */
const ICON_PATHS: Record<CardIcon, ReactNode> = {
  info: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 11v6M12 7.5v.5" />
    </>
  ),
  check: <path d="M5 12.5l4.5 4.5L19 7.5" />,
  warning: (
    <>
      <path d="M12 3.5l9.5 17h-19z" />
      <path d="M12 10v5M12 17.5v.5" />
    </>
  ),
  error: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M9 9l6 6M15 9l-6 6" />
    </>
  ),
  bell: (
    <>
      <path d="M6 16V11a6 6 0 0112 0v5l1.5 2h-15z" />
      <path d="M10 20.5a2 2 0 004 0" />
    </>
  ),
  chart: <path d="M4 18l5-6 4 3 7-9M15 6h5v5" />,
  server: (
    <>
      <rect x="4" y="4" width="16" height="7" rx="1.5" />
      <rect x="4" y="13" width="16" height="7" rx="1.5" />
      <path d="M8 7.5h.01M8 16.5h.01" />
    </>
  ),
  bot: (
    <>
      <rect x="5" y="8" width="14" height="11" rx="2" />
      <path d="M12 8V4.5M9.5 13h.01M14.5 13h.01M9.5 16h5" />
    </>
  ),
};

function Icon({ name }: { name: CardIcon }) {
  return (
    <span className="message-card__icon" aria-hidden="true">
      <svg
        viewBox="0 0 24 24"
        width="16"
        height="16"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        {ICON_PATHS[name]}
      </svg>
    </span>
  );
}

interface CardImageProps {
  url: string;
  title: string;
  previews: boolean;
  tabIndex: number;
  onImageClick?: ImageClickHandler | undefined;
}

function CardImage({ url, title, previews, tabIndex, onImageClick }: CardImageProps) {
  const { t } = useTranslation("chat");
  // `undefined` while loading, `null` when there is nothing to show.
  const [image, setImage] = useState<InlineImage | null | undefined>(undefined);
  useEffect(() => {
    if (!previews) return;
    let active = true;
    void loadInlineImage(url).then((loaded) => {
      if (active) setImage(loaded);
    });
    return () => {
      active = false;
    };
  }, [url, previews]);
  const safe = previews && image !== null && image !== undefined && SAFE_SRC.test(image.src);
  const src = useDisplaySrc(safe ? image.src : undefined);
  if (previews && image === undefined) {
    return (
      <div
        className="message-card__image message-card__image--loading"
        role="img"
        aria-busy="true"
        aria-label={t("image.loading", { name: title })}
      />
    );
  }
  if (!previews || image === null || image === undefined || !SAFE_SRC.test(image.src)) {
    // Previews off, too large or unreachable: the image stays a link, as in official clients.
    return (
      <a className="message-card__image-link" href={url} tabIndex={tabIndex}>
        {t("card.imageLink", { url: displayUrl(url) })}
      </a>
    );
  }
  return (
    <button
      type="button"
      className="message-card__image"
      tabIndex={tabIndex}
      aria-label={t("image.view", { name: title })}
      onClick={(event) => {
        if (onImageClick) onImageClick(image, event.currentTarget);
        else openInBrowser(image.url);
      }}
    >
      <img src={src} alt="" draggable={false} width={image.width} height={image.height} />
    </button>
  );
}

interface MessageCardProps {
  card: Card;
  /** Roving tab stop of the message. */
  tabIndex: number;
  /** "Show link previews and embeds": only then the image is fetched. */
  previews: boolean;
  onImageClick?: ImageClickHandler | undefined;
}

export function MessageCard({ card, tabIndex, previews, onImageClick }: MessageCardProps) {
  const { t } = useTranslation("chat");
  const body = useMemo(() => highlightCode(card.body), [card.body]);
  const bodyRef = useRef<HTMLDivElement>(null);
  const codeActions = useCodeBlockActions(bodyRef, body, tabIndex);
  const status = card.color === null ? null : t(`card.status.${card.color}`);
  const title = card.title ?? t("card.untitled");
  const label =
    status === null ? t("card.label", { title }) : t("card.labelWithStatus", { title, status });
  return (
    <section className="message-card" data-color={card.color ?? undefined} aria-label={label}>
      {card.title !== null || card.icon !== null || card.color !== null ? (
        <div className="message-card__header">
          {card.icon !== null ? <Icon name={card.icon} /> : null}
          {card.title === null ? null : card.url === null ? (
            <p className="message-card__title">{card.title}</p>
          ) : (
            <a className="message-card__title" href={card.url} tabIndex={tabIndex}>
              {card.title}
            </a>
          )}
          {card.color !== null && status !== null ? (
            <span className="message-card__status">
              <span aria-hidden="true">{STATUS_SYMBOL[card.color]}</span> {status}
            </span>
          ) : null}
        </div>
      ) : null}
      {body === "" ? null : (
        <div
          ref={bodyRef}
          className="message-card__body chat-message__body"
          dangerouslySetInnerHTML={{ __html: body }}
        />
      )}
      {codeActions}
      {card.image === null ? null : (
        <CardImage
          url={card.image}
          title={card.title ?? imageName(card.image)}
          previews={previews}
          tabIndex={tabIndex}
          onImageClick={onImageClick}
        />
      )}
      {card.url === null ? null : (
        <div className="message-card__link">
          <a href={card.url} tabIndex={tabIndex}>
            {displayUrl(card.url)}
          </a>
          <button
            type="button"
            className="message-card__open"
            tabIndex={tabIndex}
            aria-label={t("card.openLabel", { title })}
            onClick={() => {
              openInBrowser(card.url ?? "");
            }}
          >
            {t("card.open")} <span aria-hidden="true">↗</span>
          </button>
        </div>
      )}
    </section>
  );
}

/** The cards of one message, in order. */
export function MessageCards({
  cards,
  ...rest
}: Omit<MessageCardProps, "card"> & { cards: readonly Card[] }) {
  if (cards.length === 0) return null;
  return (
    <div className="message-cards">
      {cards.map((card, index) => (
        // Cards never change or reorder within a message, so the position is a stable key.
        <MessageCard key={index} card={card} {...rest} />
      ))}
    </div>
  );
}
