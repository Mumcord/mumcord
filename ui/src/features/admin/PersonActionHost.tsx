/**
 * Renders the open kick or ban dialog of a person (Epic 4, issue #253, FR-10.11, FR-10.12). Mounted
 * once, next to the person menu that opens it; each title carries the server name (FR-10.58).
 */
import { useTranslation } from "react-i18next";
import { displayName } from "../servers/format";
import { useServers } from "../servers/store";
import { BanDialog } from "./BanDialog";
import { KickDialog } from "./KickDialog";
import { ResetDialog } from "./ResetDialog";
import { usePersonAction } from "./personActionStore";

export function PersonActionHost() {
  const { t } = useTranslation(["admin", "servers"]);
  const action = usePersonAction();
  const servers = useServers();
  if (!action) return null;
  const server = servers.servers.find((s) => s.id === action.serverId);
  const serverName = server ? displayName(server) : t("servers:unknownServer");
  switch (action.mode) {
    case "kick":
      return <KickDialog action={action} serverName={serverName} />;
    case "ban":
      return <BanDialog action={action} serverName={serverName} />;
    case "resetComment":
    case "resetAvatar":
      return <ResetDialog action={action} serverName={serverName} />;
  }
}
