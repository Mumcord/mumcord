import { useTranslation } from "react-i18next";
import { CertTrustDialog } from "./CertTrustDialog";
import { ConfirmDialog } from "./ConfirmDialog";
import { displayName } from "./format";
import { ServerBar } from "./ServerBar";
import { ServerDialog } from "./ServerDialog";
import {
  closeDialog,
  confirmSwitch,
  decideTrust,
  removeServer,
  useServers,
  type ServersState,
} from "./store";
import "./servers.css";

function nameOf(state: ServersState, id: number): string {
  const server = state.servers.find((s) => s.id === id);
  return server ? displayName(server) : "";
}

function ServerDialogs() {
  const { t } = useTranslation("servers");
  const state = useServers();
  const { dialog, connection } = state;
  if (!dialog) return null;
  switch (dialog.kind) {
    case "add":
      return (
        <ServerDialog
          server={null}
          link={dialog.link}
          identityName={state.identityName}
          onClose={closeDialog}
        />
      );
    case "edit": {
      const server = state.servers.find((s) => s.id === dialog.serverId);
      if (!server) return null;
      return (
        <ServerDialog
          key={server.id}
          server={server}
          link={null}
          identityName={state.identityName}
          onClose={closeDialog}
        />
      );
    }
    case "remove":
      return (
        <ConfirmDialog
          title={t("remove.title", { server: nameOf(state, dialog.serverId) })}
          message={t("remove.message")}
          confirmLabel={t("remove.confirm")}
          danger
          onConfirm={() => {
            closeDialog();
            void removeServer(dialog.serverId);
          }}
          onClose={closeDialog}
        />
      );
    case "switch": {
      const current = connection.state === "disconnected" ? null : connection.serverId;
      return (
        <ConfirmDialog
          title={t("switch.title")}
          message={t("switch.message", {
            current: current === null ? "" : nameOf(state, current),
            next: nameOf(state, dialog.serverId),
          })}
          confirmLabel={t("switch.confirm")}
          onConfirm={() => {
            confirmSwitch(dialog.serverId, dialog.channelPath);
          }}
          onClose={closeDialog}
        />
      );
    }
  }
}

/** Content of the server bar mount: the bar plus the server, confirmation and trust dialogs. */
export function ServersRoot() {
  const state = useServers();
  const prompt = state.certPrompt;
  return (
    <>
      <ServerBar />
      <ServerDialogs />
      {prompt && (
        <CertTrustDialog
          key={prompt.fingerprint}
          prompt={prompt}
          serverName={nameOf(state, prompt.serverId)}
          onDecide={(trust) => void decideTrust(trust)}
        />
      )}
    </>
  );
}
