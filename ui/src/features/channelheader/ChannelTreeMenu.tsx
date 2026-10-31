import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { announce } from "../../a11y/announcer";
import type { ChannelMenuRequest } from "../channels";
import { ServerMenu } from "../servers/ServerMenu";
import { useChannelActions } from "./channelActions";

/**
 * A channel's right-click menu in the tree (issue #172): the same actions as the channel-name
 * menu in the chat header, plus "Join channel" and "Move users here…". Stays mounted while a
 * dialog it opened is shown.
 */
export function ChannelTreeMenu({
  channelId,
  position,
  join,
  restoreFocus,
  onDone,
}: ChannelMenuRequest) {
  const [open, setOpen] = useState(true);
  const actions = useChannelActions(channelId, {
    context: "tree",
    closeMenu: (returnFocus) => {
      setOpen(false);
      if (returnFocus) restoreFocus();
    },
    restoreFocus,
    // The tree has no status line of its own; "Link copied" goes to the live region.
    onStatus: announce,
    join,
  });
  const done = !open && actions.dialog === null;

  useEffect(() => {
    if (done) onDone();
  }, [done, onDone]);

  return (
    <>
      {open &&
        createPortal(
          <ServerMenu
            label={actions.label}
            position={position}
            onClose={(returnFocus) => {
              setOpen(false);
              if (returnFocus) restoreFocus();
            }}
            items={actions.items}
          />,
          document.body,
        )}
      {actions.dialog}
    </>
  );
}
