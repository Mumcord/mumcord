import { SettingsImport } from "../mumbleImport/SettingsImport";
import { WhisperTargetsGroup } from "../whisper/WhisperTargetsGroup";
import { LicencesCard, SetupCard, SupportCard, VersionCard } from "./sections/AboutSection";
import { AccessibilityCard } from "./sections/AccessibilitySection";
import { AcceptedConversationsGroup } from "./sections/AcceptedConversationsGroup";
import { ChannelListCard, LookCard, StartupCard, ThemeCard } from "./sections/AppearanceSection";
import { DevicesCard, MicTestCard } from "./sections/AudioSection";
import { BlockedMessagesGroup, BlockedUsersGroup } from "./sections/BlockedUsersGroup";
import { BetaFeaturesCard } from "./sections/BetaFeaturesCard";
import { CacheGroup } from "./sections/CacheGroup";
import {
  ExternalContentCard,
  HistorySyncCard,
  MessageHistoryCard,
  PerServerHistoryCard,
} from "./sections/ChatHistorySection";
import { DefaultAvatarGroup } from "./sections/DefaultAvatarGroup";
import { DiagnosticsCard } from "./sections/DiagnosticsSection";
import { DndGroup } from "./sections/DndGroup";
import { DuckingGroup } from "./sections/DuckingGroup";
import { GamePluginsGroup } from "./sections/GamePluginsGroup";
import { IdentitiesCard } from "./sections/IdentitySection";
import { LanguageCard } from "./sections/LanguageSection";
import { NowGamingGroup } from "./sections/NowGamingGroup";
import { NowPlayingGroup } from "./sections/NowPlayingGroup";
import { InGameOverlayCard } from "./sections/OverlaySection";
import { DeveloperToolsCard } from "./sections/PacketInspectorSection";
import { PositionalGroup } from "./sections/PositionalGroup";
import { ConnectionsCard } from "./sections/PrivacySection";
import { PrivacySummaryCard } from "./sections/PrivacySummaryCard";
import { PrivateMessagesGroup } from "./sections/PrivateMessagesGroup";
import { ServerConnectionsCard } from "./sections/ServersSection";
import { NotificationsCard, SoundsCard } from "./sections/SoundsSection";
import type { SettingsSectionProps } from "./sections/types";
import { UpdatesCard } from "./sections/UpdatesSection";
import { ConnectingCard, ProcessingCard, TalkModeCard } from "./sections/VoiceSection";
import type { SectionId } from "./sectionIds";
import type { PresetId } from "../wizard/presets";

interface SettingsPageProps extends SettingsSectionProps {
  id: SectionId;
  /** macOS has no in-game overlay (QA.58). */
  macos: boolean;
  /** The developer tools card stays hidden until they are unlocked (issue #60). */
  devTools: boolean;
  /** The preset all preset-covered values match right now, for the privacy summary. */
  presetMatch: PresetId | null;
}

/** The section cards of one settings group (#161); keep `settingsIndex` in step. */
export function SettingsPageCards({
  id,
  settings,
  update,
  macos,
  devTools,
  presetMatch,
}: SettingsPageProps) {
  const props = { settings, update };
  switch (id) {
    case "general":
      return (
        <>
          <StartupCard {...props} />
          <LanguageCard {...props} />
          <UpdatesCard {...props} />
          <AccessibilityCard {...props} />
        </>
      );
    case "account":
      return (
        <>
          <IdentitiesCard />
          <DefaultAvatarGroup {...props} />
          <NowPlayingGroup {...props} />
          <NowGamingGroup {...props} />
        </>
      );
    case "voiceAudio":
      return (
        <>
          <DevicesCard {...props} />
          <MicTestCard />
          <TalkModeCard {...props} />
          <ProcessingCard {...props} />
          <WhisperTargetsGroup />
          <ConnectingCard {...props} />
          <DuckingGroup {...props} />
          <SoundsCard {...props} />
        </>
      );
    case "chatNotifications":
      return (
        <>
          <NotificationsCard {...props} />
          <DndGroup />
          <PrivateMessagesGroup {...props} />
          <AcceptedConversationsGroup />
          <BlockedUsersGroup />
          <BlockedMessagesGroup {...props} />
          <MessageHistoryCard {...props} />
        </>
      );
    case "privacy":
      return (
        <>
          <PrivacySummaryCard settings={settings} presetMatch={presetMatch} />
          <ConnectionsCard {...props} />
          <ExternalContentCard {...props} />
          <HistorySyncCard {...props} />
        </>
      );
    case "appearance":
      return (
        <>
          <ThemeCard {...props} />
          <LookCard {...props} />
          <ChannelListCard {...props} />
        </>
      );
    case "servers":
      return (
        <>
          <ServerConnectionsCard {...props} />
          <SettingsImport {...props} />
          <PerServerHistoryCard />
        </>
      );
    case "gaming":
      return (
        <>
          {!macos && <InGameOverlayCard />}
          <PositionalGroup {...props} />
          <GamePluginsGroup />
        </>
      );
    case "advanced":
      return (
        <>
          <BetaFeaturesCard {...props} />
          {devTools && <DeveloperToolsCard />}
          <DiagnosticsCard {...props} />
          <CacheGroup />
          <LicencesCard />
        </>
      );
    case "about":
      return (
        <>
          <VersionCard {...props} />
          <SupportCard />
          <SetupCard />
        </>
      );
  }
}
