import { useTranslation } from "react-i18next";
import type { User } from "../../api";
import { useShownServerId } from "../channels/serverStore";
import { BlockIcon } from "./BlockIcon";
import { useBlock } from "./blockedStore";
import "./blocking.css";

/**
 * Marks a person the user blocked, in the member list and the channel tree of the shown server.
 * Only the user sees it; the label makes it more than a colour for screen readers.
 */
export function BlockedBadge({ user }: { user: Pick<User, "name" | "certHash"> }) {
  const { t } = useTranslation("members");
  const serverId = useShownServerId();
  const block = useBlock(serverId, user.certHash, user.name);
  if (!block) return null;
  const label = t("block.badge");
  // The leading space keeps the mark a separate word in the row's accessible name.
  return (
    <>
      {" "}
      <span className="blocked-badge" data-tooltip={label} role="img" aria-label={label}>
        <BlockIcon size={13} />
      </span>
    </>
  );
}
