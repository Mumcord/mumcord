import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import { ContextMenu, type MenuItem, type MenuPosition } from "../../components/ContextMenu";
import { selectConversation, useServerState } from "../channels";
import { channelPath } from "../channelheader/channelModel";

/** Door with an arrow in. */
const JOIN_ICON = "M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4M10 17l5-5-5-5M15 12H3";
/** A list with a marker. */
const SHOW_ICON = "M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01";
/** Two sheets. */
const COPY_ICON = "M9 9h11v11H9zM5 15H4V4h11v1";

/**
 * The menu of a `#Channel` chip in a message (issue #643): Join, Show in channel list and Copy
 * name. Join goes the same way as from the channel tree, so the server's rights and a channel
 * password apply as usual. When several channels share the name, each gets its own Join and Show
 * line with its path.
 */
export function ChannelMentionMenu({
  channelIds,
  menu,
  onClose,
}: {
  channelIds: readonly number[];
  menu: MenuPosition;
  onClose: () => void;
}) {
  const { t } = useTranslation("chat");
  const server = useServerState();
  const own = server.selfSession === null ? undefined : server.users.get(server.selfSession);
  const channels = channelIds.map((id) => server.channels.get(id)).filter((c) => c !== undefined);
  const first = channels[0];
  if (first === undefined || server.serverId === null) return null;
  const serverId = server.serverId;
  const several = channels.length > 1;

  const items: MenuItem[] = channels.flatMap((channel) => {
    const path = channelPath(server.channels, channel.id).join(" › ");
    const where = several ? path : channel.name;
    // Hidden when it cannot work: already there, or no right to enter and no password to ask for.
    const canJoin =
      own?.channelId !== channel.id && (channel.permissions.enter || channel.enterRestricted);
    return [
      ...(canJoin
        ? [
            {
              label: several
                ? t("channelMention.joinPath", { path: where })
                : t("channelMention.join"),
              icon: JOIN_ICON,
              onSelect: () => {
                // A refused join asks for the channel password, as from the tree (FR-2.78).
                getApi()
                  .channelJoin(serverId, channel.id)
                  .catch((error: unknown) => {
                    console.error("Joining a mentioned channel failed", error);
                  });
              },
            },
          ]
        : []),
      {
        label: several ? t("channelMention.showPath", { path: where }) : t("channelMention.show"),
        icon: SHOW_ICON,
        onSelect: () => {
          selectConversation({ kind: "channel", channelId: channel.id });
        },
      },
    ];
  });
  items.push({
    label: t("channelMention.copy"),
    icon: COPY_ICON,
    separatorBefore: true,
    onSelect: () => {
      getApi()
        .copyText(`#${first.name}`)
        .catch((error: unknown) => {
          console.error("Copying a channel name failed", error);
        });
    },
  });

  return (
    <ContextMenu
      menu={menu}
      label={t("channelMention.menu", { channel: first.name })}
      header={<span className="server-menu__header">#{first.name}</span>}
      items={items}
      onClose={onClose}
    />
  );
}
