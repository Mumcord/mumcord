/**
 * The sync pill of the cinema view (issue #163): how many people are not in step. The dot keeps the
 * meaning without colour alone, and the count is text.
 */
import { useTranslation } from "react-i18next";
import { outOfSync, type SyncState } from "./syncState";

export function SyncPill({ states }: { states: readonly SyncState[] }) {
  const { t } = useTranslation("common");
  const out = outOfSync(states);
  return (
    <span className="sync-pill" data-out={out > 0 || undefined}>
      <span className="sync-pill__dot" aria-hidden="true" />
      {out === 0 ? t("media.inSyncAll") : t("media.inSyncExcept", { count: out })}
    </span>
  );
}
