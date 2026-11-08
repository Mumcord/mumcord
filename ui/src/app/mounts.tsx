/**
 * Mount points for feature UIs. Each feature task replaces the placeholder in its own mount
 * with its root component; the shell layout itself stays untouched.
 */
import { lazy } from "react";
import { useTranslation } from "react-i18next";
import { LazyBoundary } from "../components/LazyBoundary";
import { getApi } from "../api";
import { useView } from "./view";
import { useServerStateOf, useShownServerId, useVoiceServerId } from "../features/channels";
import { useMembersColumn } from "./membersCollapse";
import { useActionKey } from "../features/hotkeys/defaultHotkeys";
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
import { PluginActivityLines, PluginMiniBars, PluginsHost } from "../features/plugins";
import { SidePanelHost } from "../features/plugins/sidepanel";
import { UserStatsWatcher } from "../features/connection";
import { FriendsList } from "../features/friends";
import { MembersTabs } from "../features/activity/MembersTabs";
import {
  MembersPanel,
  MembersToggle,
  openPrivateConversation,
  useChannelPresence,
} from "../features/members";
import { ShareMiniBar, SharerBar } from "../features/screenshare";
import { CameraMiniBar, CameraRoomSync } from "../features/camera/room";
import {
  VideoDucking,
  WatchHeaderActions,
  WatchHeaderTitle,
  WatchMiniBar,
} from "../features/watchparty";
import { MediaArea } from "../features/media/MediaArea";
import { MediaPopoutHost } from "../features/media/MediaPopoutHost";
import { useMediaView } from "../features/media/mediaView";
import { StagePanel } from "../features/media/StagePanel";
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
import { CrashNotice } from "../features/app/CrashNotice";
import { WhisperBanner } from "../features/whisper/WhisperBanner";

// Screens the first frame does not need load as chunks of their own (issue #323).
const SettingsScreen = lazy(() =>
  import("../features/settings/SettingsScreen").then((m) => ({ default: m.SettingsScreen })),
);
const Wizard = lazy(() => import("../features/wizard/Wizard").then((m) => ({ default: m.Wizard })));
const WatchTogetherView = lazy(() =>
  import("../features/watchparty/WatchTogetherView").then((m) => ({
    default: m.WatchTogetherView,
  })),
);
const WatchPartyPanel = lazy(() =>
  import("../features/watchparty/WatchPartyPanel").then((m) => ({ default: m.WatchPartyPanel })),
);
const CameraRoomPanel = lazy(() =>
  import("../features/camera/room/CameraRoomPanel").then((m) => ({ default: m.CameraRoomPanel })),
);
const ScreenShareView = lazy(() =>
  import("../features/screenshare/ScreenShareView").then((m) => ({ default: m.ScreenShareView })),
);
const ViewerPanel = lazy(() =>
  import("../features/screenshare/ViewerPanel").then((m) => ({ default: m.ViewerPanel })),
);

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
      <CrashNotice />
      <Updater />
      <PluginsHost />
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
  // One resizable media area for every item of the stage (spec 07 §4.9, FR-9.44). All of them
  // stay mounted while the area is open, so switching tabs never stops one.
  const media =
    serverId === null || view.cinema === null ? null : (
      <MediaArea>
        {view.stage.includes("watch") && (
          <StagePanel id="watch">
            <LazyBoundary>
              <WatchTogetherView serverId={serverId} />
            </LazyBoundary>
          </StagePanel>
        )}
        {view.stage.includes("share") && (
          <StagePanel id="share">
            <LazyBoundary>
              <ScreenShareView />
            </LazyBoundary>
          </StagePanel>
        )}
        {view.stage.includes("camera") && (
          <StagePanel id="camera">
            <LazyBoundary>
              <CameraRoomPanel serverId={serverId} onTurnOn={null} />
            </LazyBoundary>
          </StagePanel>
        )}
      </MediaArea>
    );
  // The channel header always stays at the very top (issue #163): the sharer bar, the mini bars
  // and the cinema view all sit below it, and focus keeps it while hiding the messages.
  const bars =
    serverId === null ? null : (
      <>
        <SharerBar />
        {view.mini.watch && <WatchMiniBar serverId={serverId} />}
        {view.mini.share && <ShareMiniBar serverId={serverId} />}
        <CameraMiniBar serverId={serverId} />
        <CameraRoomSync serverId={serverId} />
        <PluginMiniBars />
        <PluginActivityLines />
      </>
    );
  return (
    <ChatView
      renderChannelTitle={(channelId) =>
        serverId === null ? (
          <ChannelHeader channelId={channelId} />
        ) : (
          <WatchHeaderTitle serverId={serverId}>
            <ChannelHeader channelId={channelId} />
          </WatchHeaderTitle>
        )
      }
      renderHeaderActions={(conv) => (
        <>
          {serverId !== null && <WatchHeaderActions serverId={serverId} />}
          <FollowHeaderActions conversation={conv} />
          <MembersToggleMount />
          <NotificationBell />
        </>
      )}
      belowHeader={
        bars === null && media === null ? undefined : (
          <>
            {bars}
            {media}
          </>
        )
      }
    />
  );
}
/** The members button in the chat header, wired to the shell's member column (issue #165). */
function MembersToggleMount() {
  const column = useMembersColumn();
  const presence = useChannelPresence();
  const shortcut = useActionKey("toggleMemberColumn");
  if (column === null) return null;
  return (
    <MembersToggle
      expanded={column.shown}
      controls={column.controls}
      presence={presence}
      shortcut={shortcut}
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
      <MediaPopoutHost />
    </>
  );
}
/** Member list (B20) with the friends list below (FR-1.64); the party's panel takes the column
while its cinema view is open (issue #163). Without cinema the column is the Members | Activity
tab strip of the activity log (FR-11.1). */
export function MembersMount() {
  return (
    <SidePanelHost>
      <MembersColumn />
    </SidePanelHost>
  );
}
function MembersColumn() {
  const view = useMediaView();
  const serverId = useVoiceServerId();
  const shown = useShownServerId();
  const { users, selfSession } = useServerStateOf(serverId);
  const own = selfSession === null ? undefined : users.get(selfSession);
  if (view.cinema === "watch" && serverId !== null && own !== undefined) {
    return (
      <LazyBoundary>
        <WatchPartyPanel serverId={serverId} channelId={own.channelId} />
      </LazyBoundary>
    );
  }
  if (view.cinema === "share") {
    return (
      <LazyBoundary>
        <ViewerPanel />
      </LazyBoundary>
    );
  }
  return (
    <MembersTabs
      // The column lists the people of the server it shows, so the feed belongs to that server.
      serverId={shown}
      members={
        <>
          <MembersPanel onSendPrivateMessage={openPrivateConversation} />
          <FriendsList />
        </>
      }
    />
  );
}
/** First-start wizard content (B18); calls `useView().finishWizard()` when done. */
export function WizardMount() {
  const { t } = useTranslation("app");
  return (
    <>
      {/* The rail shows the app name; the heading names the screen for assistive technology. */}
      <h1 className="visually-hidden">{t("wizard.title")}</h1>
      <LazyBoundary>
        <Wizard />
      </LazyBoundary>
    </>
  );
}
/** Settings content inside the settings overlay (B24). */
export function SettingsMount() {
  const { settingsSection, settingsRevision } = useView();
  // A section requested while the settings are already open (e.g. "Manage identities" in the
  // About page or the wizard rerun, #149) remounts the screen there.
  return (
    <LazyBoundary>
      <SettingsScreen
        key={settingsSection ?? "default"}
        settingsRevision={settingsRevision}
        {...(settingsSection ? { initialSection: settingsSection } : {})}
      />
    </LazyBoundary>
  );
}
