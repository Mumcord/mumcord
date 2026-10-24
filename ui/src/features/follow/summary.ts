import type { TFunction } from "i18next";
import type { FollowAllSummary } from "../../api";

/** "Followed 42 channels. 8 skipped: limit reached, not allowed." */
export function describeSummary(t: TFunction<"follow">, summary: FollowAllSummary): string {
  const reasons = [
    summary.limitReached > 0 ? t("all.limit") : null,
    summary.notAllowed > 0 ? t("all.notAllowed") : null,
    summary.noAnswer > 0 ? t("all.noAnswer") : null,
  ].filter((r): r is string => r !== null);
  const skipped = summary.limitReached + summary.notAllowed + summary.noAnswer;
  const followed = t("all.followed", { count: summary.followed });
  return skipped === 0
    ? followed
    : `${followed} ${t("all.skipped", { count: skipped, reasons: reasons.join(", ") })}`;
}
