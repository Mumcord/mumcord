/**
 * Pinned, collapsible box at the top of a channel's chat with the channel description and the
 * server's welcome text (FR-1.48b). Long content starts collapsed to about three lines.
 */
import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import "./pinnedInfoClose.css";
import { openLink } from "./openLink";
import { sanitizeHtml } from "./sanitize";

/** Links inside the sanitized HTML open in the system browser, like in messages. */
function onLinkClick(event: MouseEvent) {
  const link = event.target instanceof Element ? event.target.closest("a") : null;
  if (!link) return;
  event.preventDefault();
  const href = link.getAttribute("href");
  if (href) {
    openLink(href).catch((error: unknown) => {
      console.error("Opening a link failed", error);
    });
  }
}

const BLOCK_SEPARATOR = "\u0000";

export interface PinnedBlock {
  /** Untrusted server HTML; sanitized here. */
  html: string;
  /** Set when the block can be closed (welcome text, channel description); label of the button. */
  hideLabel?: string;
  onHide?: () => void;
}

export interface PinnedInfoBoxProps {
  blocks: readonly PinnedBlock[];
}

export function PinnedInfoBox({ blocks }: PinnedInfoBoxProps) {
  const { t } = useTranslation("chat");
  const bodyId = useId();
  const bodyRef = useRef<HTMLDivElement>(null);
  const [expanded, setExpanded] = useState(false);
  const [overflows, setOverflows] = useState(false);
  // Keyed by content: the caller derives a new array on every server state change.
  const joined = blocks.map((block) => block.html).join(BLOCK_SEPARATOR);
  const sanitized = useMemo(
    () =>
      joined
        .split(BLOCK_SEPARATOR)
        .map((html) => sanitizeHtml(html))
        .filter((html) => html.trim() !== ""),
    [joined],
  );
  // Blocks that sanitize to nothing are skipped, so pair the survivors with their close buttons.
  const shown = blocks
    .map((block) => ({ block, html: sanitizeHtml(block.html) }))
    .filter((entry) => entry.html.trim() !== "");

  // Only offer "Show more" when the collapsed box actually hides something.
  useLayoutEffect(() => {
    const body = bodyRef.current;
    if (!body) return;
    const measure = () => {
      if (expanded) return;
      setOverflows(body.scrollHeight > body.clientHeight + 1);
    };
    measure();
    window.addEventListener("resize", measure);
    return () => {
      window.removeEventListener("resize", measure);
    };
  }, [sanitized, expanded]);

  const hasContent = sanitized.length > 0;
  useEffect(() => {
    const body = bodyRef.current;
    if (!body) return;
    body.addEventListener("click", onLinkClick);
    return () => {
      body.removeEventListener("click", onLinkClick);
    };
  }, [hasContent]);

  if (!hasContent) return null;
  const collapsed = !expanded;

  return (
    <section className="pinned-info" aria-label={t("pinned.label")}>
      <div
        ref={bodyRef}
        id={bodyId}
        className={
          collapsed ? "pinned-info__body pinned-info__body--collapsed" : "pinned-info__body"
        }
      >
        {shown.map(({ block, html }, index) => (
          <div
            // The blocks are a fixed list (description, welcome text) that never reorders.
            key={index}
            className="pinned-info__block"
          >
            <div
              className="pinned-info__text chat-message__body"
              dangerouslySetInnerHTML={{ __html: html }}
            />
            {block.onHide ? (
              <button
                type="button"
                className="pinned-info__close"
                aria-label={block.hideLabel}
                data-tooltip={block.hideLabel}
                onClick={block.onHide}
              >
                ×
              </button>
            ) : null}
          </div>
        ))}
      </div>
      {overflows || expanded ? (
        <button
          type="button"
          className="pinned-info__toggle"
          aria-expanded={expanded}
          aria-controls={bodyId}
          onClick={() => {
            setExpanded((value) => !value);
          }}
        >
          {expanded ? t("pinned.showLess") : t("pinned.showMore")}
        </button>
      ) : null}
    </section>
  );
}
