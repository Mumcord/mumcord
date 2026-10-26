import { useTranslation } from "react-i18next";
import { useServerStateOf } from "../channels/serverStore";
import { CertTrustDialog } from "./CertTrustDialog";
import { ChannelPasswordDialog } from "./ChannelPasswordDialog";
import { useChannelPasswordPrompt } from "./channelPasswordStore";
import { ConfirmDialog } from "./ConfirmDialog";
import { displayName } from "./format";
import { PasswordDialog } from "./PasswordDialog";
import { ServerBar } from "./ServerBar";
import { ServerDialog } from "./ServerDialog";
import { ServerThemeDialog } from "./ServerThemeDialog";
import { closeDialog, decideTrust, removeServer, useServers, type ServersState } from "./store";
import "./servers.css";

function nameOf(state: ServersState, id: number): string {
  const server = state.servers.find((s) => s.id === id);
  return server ? displayName(server) : "";
}

function ServerDialogs() {
  const { t } = useTranslation("servers");
  const state = useServers();
  const { dialog } = state;
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
    case "theme": {
      const server = state.servers.find((s) => s.id === dialog.serverId);
      if (!server) return null;
      return <ServerThemeDialog key={server.id} server={server} onClose={closeDialog} />;
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
  }
}

function ChannelPasswordHost() {
  const prompt = useChannelPasswordPrompt();
  const server = useServerStateOf(prompt?.serverId ?? null);
  if (!prompt) return null;
  return (
    <ChannelPasswordDialog
      key={`${String(prompt.serverId)}-${String(prompt.channelId)}`}
      prompt={prompt}
      channelName={server.channels.get(prompt.channelId)?.name ?? ""}
    />
  );
}

/** Content of the server bar mount: the bar plus the server, confirmation and trust dialogs. */
export function ServersRoot() {
  const state = useServers();
  const prompt = state.certPrompt;
  const firstPrompt = state.passwordPrompts[0];
  return (
    <>
      <ServerBar />
      <ServerDialogs />
      <ChannelPasswordHost />
      {firstPrompt && (
        <PasswordDialog
          key={firstPrompt.serverId}
          prompt={firstPrompt}
          serverName={nameOf(state, firstPrompt.serverId)}
          savedPassword={
            state.servers.find((s) => s.id === firstPrompt.serverId)?.hasPassword ?? false
          }
        />
      )}
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
