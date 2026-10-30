import { useTranslation } from "react-i18next";
import { ContextMenu, type MenuItem, type MenuPosition } from "../../components/ContextMenu";

/** Phone handset, as on the button; marks the menu's disconnecting actions beside their text. */
const HANG_UP = "M3 14c5-5 13-5 18 0l-2.5 2.5-3-1.5v-2.5a9 9 0 0 0-5 0V15l-3 1.5z";
const VOICE_OFF = "M12 3a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V6a3 3 0 0 0-3-3zM4 4l16 16";

export interface VoiceTarget {
  serverId: number;
  label: string;
}

export interface DisconnectMenuProps {
  menu: MenuPosition;
  /** Name of the server the phone button disconnects. */
  serverLabel: string;
  /** The shown server has voice, so it can be left without disconnecting. */
  canLeaveVoice: boolean;
  /** Other connected servers voice could move to. */
  switchTargets: readonly VoiceTarget[];
  /** Number of connected servers; "all" is offered from two. */
  connectedCount: number;
  onLeaveVoice: () => void;
  onDisconnect: () => void;
  onSwitchVoice: (serverId: number) => void;
  onDisconnectAll: () => void;
  onClose: () => void;
}

/** The phone button's menu: stop voice only, disconnect one server, move voice, or leave all. */
export function DisconnectMenu({
  menu,
  serverLabel,
  canLeaveVoice,
  switchTargets,
  connectedCount,
  onLeaveVoice,
  onDisconnect,
  onSwitchVoice,
  onDisconnectAll,
  onClose,
}: DisconnectMenuProps) {
  const { t } = useTranslation("userpanel");
  const items: MenuItem[] = [];
  if (canLeaveVoice) {
    items.push({
      label: t("disconnectMenu.leaveVoice"),
      hint: t("disconnectMenu.leaveVoiceHint"),
      icon: VOICE_OFF,
      onSelect: onLeaveVoice,
    });
  }
  items.push({
    label: t("disconnect", { server: serverLabel }),
    hint: t("disconnectMenu.disconnectHint"),
    icon: HANG_UP,
    danger: true,
    onSelect: onDisconnect,
  });
  for (const target of switchTargets) {
    items.push({
      label: t("disconnectMenu.switchTo", { server: target.label }),
      onSelect: () => {
        onSwitchVoice(target.serverId);
      },
    });
  }
  if (connectedCount > 1) {
    items.push({
      label: t("disconnectMenu.all"),
      hint: t("disconnectMenu.allHint"),
      icon: HANG_UP,
      danger: true,
      onSelect: onDisconnectAll,
    });
  }
  return (
    <ContextMenu menu={menu} label={t("disconnectMenu.label")} items={items} onClose={onClose} />
  );
}
