/**
 * The Members | Activity tab strip at the top of the member column (FR-11.1). Only the normal
 * column has it: a cinema view takes the whole column as its own panel (issue #163). On narrow
 * windows the column is the shell's overlay, and the strip simply lives inside it (FR-11.3).
 */
import { useId, useRef, type KeyboardEvent, type ReactNode } from "react";
import { useT } from "../../i18n/useT";
import { ActivityFeed } from "./ActivityFeed";
import { selectMembersTab, useMembersTab, type MembersTab } from "./membersTabStore";
import "./activity.css";

const TABS: readonly MembersTab[] = ["members", "activity"];

export interface MembersTabsProps {
  serverId: number | null;
  /** What the Members tab shows: the member list and the friends list, as the shell chose them. */
  members: ReactNode;
}

export function MembersTabs({ serverId, members }: MembersTabsProps) {
  const t = useT("activity");
  const baseId = useId();
  const tab = useMembersTab(serverId);
  const tabRefs = useRef(new Map<MembersTab, HTMLButtonElement>());
  const tabId = (which: MembersTab) => `${baseId}-tab-${which}`;
  const panelId = `${baseId}-panel`;

  // Left/Right switch tabs; Tab itself moves into the panel (§7).
  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    const index = TABS.indexOf(tab);
    const next =
      event.key === "ArrowRight" || event.key === "ArrowDown"
        ? TABS[(index + 1) % TABS.length]
        : event.key === "ArrowLeft" || event.key === "ArrowUp"
          ? TABS[(index - 1 + TABS.length) % TABS.length]
          : event.key === "Home"
            ? TABS[0]
            : event.key === "End"
              ? TABS[TABS.length - 1]
              : undefined;
    if (next === undefined) return;
    event.preventDefault();
    selectMembersTab(serverId, next);
    tabRefs.current.get(next)?.focus();
  };

  return (
    <div className="activity-tabs">
      <div className="activity-tabs__row" role="tablist" aria-label={t("tab.tabsLabel")}>
        {TABS.map((which) => (
          <button
            key={which}
            type="button"
            role="tab"
            id={tabId(which)}
            className="activity-tab"
            aria-selected={which === tab}
            aria-controls={panelId}
            tabIndex={which === tab ? 0 : -1}
            ref={(element) => {
              if (element) tabRefs.current.set(which, element);
              else tabRefs.current.delete(which);
            }}
            onKeyDown={onKeyDown}
            onClick={() => {
              selectMembersTab(serverId, which);
            }}
          >
            {t(`tab.${which}`)}
          </button>
        ))}
      </div>
      <div id={panelId} role="tabpanel" aria-labelledby={tabId(tab)} tabIndex={-1}>
        {tab === "members" ? members : <ActivityFeed serverId={serverId} />}
      </div>
    </div>
  );
}
