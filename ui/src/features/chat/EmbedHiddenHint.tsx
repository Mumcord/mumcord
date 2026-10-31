import { Trans } from "react-i18next";
import { useOpenPrivacySettings } from "../../app/view";
import type { HiddenEmbedKind } from "./embedHint";

interface EmbedHiddenHintProps {
  /** One entry per embed of the message that stays hidden; several get one combined hint. */
  kinds: readonly HiddenEmbedKind[];
  /** Roving tab stop of the message the hint belongs to. */
  tabIndex?: number;
}

/**
 * Quiet line under a message telling why an embed is missing while "Show link previews and
 * embeds" is off (issue #261). It is plain text, not a live region, so a screen reader reads it
 * with its message instead of announcing it on every render.
 */
export function EmbedHiddenHint({ kinds, tabIndex }: EmbedHiddenHintProps) {
  const openPrivacy = useOpenPrivacySettings();
  const [first] = kinds;
  if (first === undefined) return null;
  const several = kinds.length > 1;
  return (
    <p className="embed-hint">
      <svg
        className="embed-hint__icon"
        viewBox="0 0 24 24"
        width="14"
        height="14"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
        focusable="false"
      >
        <path d="M10.7 5.1A10 10 0 0 1 12 5c6.4 0 10 7 10 7a17 17 0 0 1-2.2 3.1M6.6 6.6C3.8 8.4 2 12 2 12s3.6 7 10 7a9.7 9.7 0 0 0 5.4-1.6" />
        <path d="M9.9 9.9a3 3 0 0 0 4.2 4.2M2 2l20 20" />
      </svg>
      <span>
        <Trans
          ns="chat"
          i18nKey={several ? "embedHint.several" : `embedHint.${first}`}
          count={kinds.length}
          components={{
            settings:
              openPrivacy === null ? (
                <span />
              ) : (
                <button
                  type="button"
                  className="embed-hint__link"
                  tabIndex={tabIndex}
                  onClick={openPrivacy}
                />
              ),
          }}
        />
      </span>
    </p>
  );
}
