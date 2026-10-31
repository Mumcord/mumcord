/**
 * Mount points for feature UIs. Each feature task replaces the placeholder in its own mount
 * with its root component; the shell layout itself stays untouched.
 */
import { useTranslation } from "react-i18next";
import { getApi } from "../api";
import { useIsMacos } from "../api/useIsMacos";
import { useView } from "./view";
import { useServerStateOf, useVoiceServerId } from "../features/channels";
import { useMembersColumn } from "./membersCollapse";
import { SettingsScreen } from "../features/settings/SettingsScreen";
import { Wizard } from "../features/wizard";
import { NotificationBell } from "../features/notifications";
import { UserPanel } from "../features/userpanel";
import { ChannelsPanel } from "../features/channels";
import {
  ChannelUnreadBadge,
  FollowAllMenu,
  FollowHeaderActions,
  FollowNoticeLine,
  FollowStar,
  MessageNotifications,
} from "../features/follow";
import { ChannelAudioHost } from "../features/channelaudio";
import { ChannelHeader, ChannelTreeMenu } from "../features/channelheader";
import { ChatView } from "../features/chat";
import { Updater } from "../features/updates";
import { UserStatsWatcher } from "../features/connection";
import { FriendsList } from "../features/friends";
import {
  MembersPanel,
  MembersToggle,
  openPrivateConversation,
  useChannelPresence,
} from "../features/members";
import { ShareMiniBar, ScreenShareView, SharerBar, ViewerPanel } from "../features/screenshare";
import {
  VideoDucking,
  WatchMiniBar,
  WatchPartyPanel,
  WatchTogetherView,
} from "../features/watchparty";
import { useMediaView } from "../features/media/mediaView";
import { ConnectionStatus, RejoinPrompt, ServersRoot } from "../features/servers";
import { closeServerBrowser, useServerBrowserOpen } from "../features/serverbrowser";
import { RequestsEntry, RequestsView, useRequestsView } from "../features/requests";
import {
  closeMessages,
  MessagesButton,
  MessagesOverview,
  useMessagesOpen,
} from "../features/messages";
import {
  AppImageMenuPrompt,
  DirectConsentPrompt,
  UpdateCheckPrompt,
  CloseDialog,
  WindowOverlayNotice,
  type WindowOverlayNoticeApi,
  AudioDeviceNotice,
  type AudioDeviceNoticeApi,
} from "../features/app";
<<<<<<< HEAD
import { WhisperBanner } from "../features/whisper/WhisperBanner";

=======
>>>>>>> a512cd7 (feat(screenshare): the sharer control bar at the top of the chat)
/** Server bar (B19). */
export function ServersMount() {
  const messages = useMessagesOpen();
  const browserOpen = useServerBrowserOpen();
  return (
    <ServersRoot
      top={<MessagesButton covered={browserOpen} onOpen={closeServerBrowser} />}
      topActive={messages}
      onServerClick={closeMessages}
    />
  );
}

/** Channel tree (B20), or the private messages overview while it is open (#173). */
export function ChannelsMount() {
  const messages = useMessagesOpen();
  if (messages) {
    return (
      <>
        <MessagesOverview />
        <UserStatsWatcher />
        <MessageNotifications />
      </>
    );
  }
  return (
    <>
      <ChannelsPanel
        header={<FollowAllMenu />}
        aboveTree={<RequestsEntry />}
        channelBadge={(channelId) => <ChannelUnreadBadge channelId={channelId} />}
        channelAction={(channel) => <FollowStar {...channel} />}
        channelMenu={(menu) => <ChannelTreeMenu key={menu.channelId} {...menu} />}
      />
      <FollowNoticeLine />
      <UserStatsWatcher />
      <MessageNotifications />
    </>
  );
}
/** User panel at the bottom of the channel column (B23). */
export function UserPanelMount() {
  return (
    <>
      <UserPanel />
    </>
  );
}
const windowOverlayNoticeApi: WindowOverlayNoticeApi = {
  pending: () => getApi().windowOverlayNoticePending(),
  dismiss: async () => {
    await getApi().windowOverlayNoticeDismiss();
  },
};
const audioDeviceNoticeApi: AudioDeviceNoticeApi = {
  pending: () => getApi().audioDeviceNoticePending(),
  dismiss: async () => {
    await getApi().audioDeviceNoticeDismiss();
  },
};
/**
 * Thin status bar above the chat, e.g. "Reconnecting…" (B19), the rejoin offer (QA.9) and the
 * close question (B25). All render nothing when idle.
 */
export function ChatStatusMount() {
  const { openOverlaySettings, openAudioSettings } = useView();
  return (
    <>
      <ConnectionStatus />
      <WhisperBanner />
      <RejoinPrompt />
      <CloseDialog />
      <ChannelAudioHost />
      <WindowOverlayNotice api={windowOverlayNoticeApi} onOpenSettings={openOverlaySettings} />
      <AudioDeviceNotice api={audioDeviceNoticeApi} onOpenSettings={openAudioSettings} />
      <Updater />
      <UpdateCheckPrompt />
      <AppImageMenuPrompt />
      <DirectConsentPrompt />
    </>
  );
}
/** Chat header, message list and input (B21), or the message requests while they are open (#141). */
export function ChatMount() {
  const requests = useRequestsView();
  const serverId = useVoiceServerId();
  const view = useMediaView();
  if (requests)
    return <RequestsView serverId={requests.serverId} selectedId={requests.selectedId} />;
  const chat = (
    <ChatView
      renderChannelTitle={(channelId) => <ChannelHeader channelId={channelId} />}
      renderHeaderActions={(conv) => (
        <>
          <FollowHeaderActions conversation={conv} />
          <MembersToggleMount />
          <NotificationBell />
        </>
      )}
    />
  );
  if (serverId === null) return chat;
  // The cinema view (issue #163): the player above the chat, which stays usable below it; the mini
  // bar is what remains when the view is left. Focus shows the picture alone.
  if (view.cinema === "watch") {
    return (
      <>
        <WatchTogetherView serverId={serverId} />
        {!view.focus && chat}
      </>
    );
  }
  if (view.cinema === "share") {
    return (
      <>
        <ScreenShareView />
        {!view.focus && chat}
      </>
    );
  }
  return (
    <>
      <SharerBar />
      {view.mini.watch && <WatchMiniBar serverId={serverId} />}
      {view.mini.share && <ShareMiniBar serverId={serverId} />}
      {chat}
    </>
  );
}
/** The members button in the chat header, wired to the shell's member column (issue #165). */
function MembersToggleMount() {
  const { t } = useTranslation("members");
  const column = useMembersColumn();
  const presence = useChannelPresence();
  const macos = useIsMacos();
  if (column === null) return null;
  return (
    <MembersToggle
      expanded={column.shown}
      controls={column.controls}
      presence={presence}
      shortcut={macos ? "⌘⇧M" : t("toggle.shortcut")}
      onToggle={column.toggle}
      buttonRef={column.buttonRef}
    />
  );
}
/**
 * Media over the main area: the watched screen share widget (FR-2.70, until its own view, issue
 * #163) and the ducking while people talk. At shell level so they stay open while switching chats
 * or servers.
 */
export function MediaWidgetsMount() {
  return (
    <>
      <VideoDucking />
    </>
  );
}
/** Member list (B20) with the friends list below (FR-1.64); the party's panel takes the column
while its cinema view is open (issue #163). */
export function MembersMount() {
  const view = useMediaView();
  const serverId = useVoiceServerId();
  const { users, selfSession } = useServerStateOf(serverId);
  const own = selfSession === null ? undefined : users.get(selfSession);
  if (view.cinema === "watch" && serverId !== null && own !== undefined) {
    return <WatchPartyPanel serverId={serverId} channelId={own.channelId} />;
  }
  if (view.cinema === "share") return <ViewerPanel />;
  return (
    <>
      <MembersPanel onSendPrivateMessage={openPrivateConversation} />
      <FriendsList />
    </>
  );
}
/** First-start wizard content (B18); calls `useView().finishWizard()` when done. */
export function WizardMount() {
  const { t } = useTranslation("app");
  return (
    <>
      <h1>{t("wizard.title")}</h1>
      <Wizard />
    </>
  );
}
/** Settings content inside the settings overlay (B24). */
export function SettingsMount() {
  const { settingsSection, settingsRevision } = useView();
  // A section requested while the settings are already open (e.g. "Manage identities" in the
  // About page or the wizard rerun, #149) remounts the screen there.
  return (
    <SettingsScreen
      key={settingsSection ?? "default"}
      settingsRevision={settingsRevision}
      {...(settingsSection ? { initialSection: settingsSection } : {})}
    />
  );
}
