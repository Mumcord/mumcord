import type { Ref } from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import type { ChannelPresence } from "./channelPresence";
import "./membersToggle.css";

export interface MembersToggleProps {
  /** Whether the member column (or its narrow-window overlay) is shown. */
  expanded: boolean;
  /** Id of the member column. */
  controls: string;
  /** The selected channel's people; `null` in a private chat. */
  presence: ChannelPresence | null;
  /** Display text of the shortcut, e.g. "Ctrl+Shift+M". */
  shortcut: string;
  onToggle: () => void;
  buttonRef?: Ref<HTMLButtonElement>;
}

/**
 * Members button in the chat header (issue #165, variant C). Highlighted while the column is open;
 * while it is hidden it shows how many people are in the channel and a green dot while one of them
 * talks, and its tooltip names who.
 */
export function MembersToggle({
  expanded,
  controls,
  presence,
  shortcut,
  onToggle,
  buttonRef,
}: MembersToggleProps) {
  const { t } = useTranslation("members");
  const action = expanded ? t("toggle.hide") : t("toggle.show");
  const summary = expanded ? null : presence;
  const talking = summary ? talkingText(t, summary.talkers) : "";
  return (
    <button
      ref={buttonRef}
      type="button"
      className="members-toggle"
      aria-expanded={expanded}
      aria-controls={controls}
      aria-keyshortcuts="Control+Shift+M Meta+Shift+M"
      aria-label={summary ? t("toggle.showCount", { count: summary.count }) : action}
      data-tooltip={action}
      data-tooltip-placement="bottom"
      data-tooltip-shortcut={shortcut}
      data-tooltip-detail={talking}
      onClick={onToggle}
    >
      <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" focusable="false">
        <path
          d="M9 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM2 21v-1a6 6 0 0 1 6-6h2a6 6 0 0 1 6 6v1M16 3.2a4 4 0 0 1 0 7.6M18 14.3a6 6 0 0 1 4 5.7v1"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.75"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </svg>
      {summary && <span className="members-toggle__count">{summary.count}</span>}
      {summary && summary.talkers.length > 0 && <span className="members-toggle__talking" />}
    </button>
  );
}

function talkingText(t: TFunction<"members">, talkers: string[]): string {
  const [first, second] = talkers;
  if (first === undefined) return "";
  if (second === undefined) return t("toggle.talkingOne", { name: first });
  if (talkers.length === 2) return t("toggle.talkingTwo", { first, second });
  return t("toggle.talkingMore", { name: first, count: talkers.length - 1 });
}
