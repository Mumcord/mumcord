import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { LiveRegions } from "../a11y";
import { TooltipLayer } from "../components/Tooltip";
import { PersonMenuHost, ProfilePreviewHost } from "../features/members";
import { NotificationsHost } from "../features/notifications";
import { ChatRequestDialogHost, RevokeConsentDialogHost } from "../features/requests";
import { WizardRerunOverlay } from "../features/wizard";
import { TraySync } from "../features/tray";
import { AppShell } from "./AppShell";
import {
  ChannelsMount,
  ChatMount,
  ChatStatusMount,
  MediaWidgetsMount,
  MembersMount,
  ServersMount,
  UserPanelMount,
  WizardMount,
} from "./mounts";
import { PreferencesProvider } from "./PreferencesProvider";
import { SettingsOverlay } from "./SettingsOverlay";
import { setSettingsOpener } from "../features/settings/settingsOpener";
import { useThemePreviewing } from "../theme/themeStore";
import { useDevToolsCapture, useDevToolsUnlocked } from "../features/inspector/devTools";
import { InspectorViewShare } from "../features/inspector/InspectorViewShare";
import { WhatsNewOnStart } from "../features/changelog/WhatsNewOnStart";
import { WindowLookShare } from "../features/licences/WindowLookShare";
import { ServerBrowser, useServerBrowserOpen } from "../features/serverbrowser";
import { useServerTheme } from "./useServerTheme";
import { useView } from "./view";
import { ViewProvider } from "./ViewProvider";

function MainScreen() {
  const { overlay, closeOverlay, openSettings, rerunWizard, closeRerunWizard } = useView();
  // "Manage identities…" and similar links elsewhere in the app land in Settings.
  useEffect(() => {
    setSettingsOpener(openSettings);
    return () => {
      setSettingsOpener(null);
    };
  }, [openSettings]);
  useServerTheme();
  useDevToolsCapture();
  const devTools = useDevToolsUnlocked();
  const previewing = useThemePreviewing();
  const browserOpen = useServerBrowserOpen();
  return (
    <>
      <AppShell
        inert={overlay !== null && !previewing}
        slots={{
          servers: <ServersMount />,
          channels: <ChannelsMount />,
          userPanel: <UserPanelMount />,
          chatStatus: <ChatStatusMount />,
          chat: <ChatMount />,
          members: <MembersMount />,
          page: browserOpen ? <ServerBrowser /> : null,
        }}
      />
      {/* Kept mounted under the settings overlay so a watched stream does not restart. */}
      <div inert={overlay !== null && !previewing}>
        <MediaWidgetsMount />
      </div>
      {overlay === "settings" && <SettingsOverlay onClose={closeOverlay} blocked={rerunWizard} />}
      {rerunWizard && (
        <WizardRerunOverlay
          onClose={closeRerunWizard}
          onManageIdentities={() => {
            closeRerunWizard();
            openSettings("identity");
          }}
        />
      )}
      {overlay === null && <ProfilePreviewHost />}
      {overlay === null && <PersonMenuHost />}
      {overlay === null && <ChatRequestDialogHost />}
      {overlay === null && <RevokeConsentDialogHost />}
      <NotificationsHost overlayOpen={overlay !== null} onOpenChat={closeOverlay} />
      <TraySync onOpenSettings={openSettings} />
      {devTools && <InspectorViewShare />}
      <WindowLookShare />
    </>
  );
}

function StartupError() {
  const { t } = useTranslation(["app", "common"]);
  const { retry } = useView();
  return (
    <main className="centered-screen" aria-labelledby="startup-error-title">
      <h1 id="startup-error-title">{t("startupError.title")}</h1>
      <p>{t("startupError.description")}</p>
      <button type="button" className="button button--primary" onClick={retry}>
        {t("common:retry")}
      </button>
    </main>
  );
}

function Screens() {
  const { t } = useTranslation("common");
  const { screen } = useView();
  switch (screen) {
    case "loading":
      return (
        <main className="centered-screen" aria-busy="true" aria-label={t("appName")}>
          <p role="status">{t("loading")}</p>
        </main>
      );
    case "startupError":
      return <StartupError />;
    case "wizard":
      return (
        <main className="centered-screen wizard-frame" aria-label={t("appName")}>
          <WizardMount />
        </main>
      );
    case "main":
      return <MainScreen />;
  }
}

/**
 * Work for each start once the settings have loaded. Stays mounted from the wizard to the main
 * screen, so it runs once and sees whether this was a first start.
 */
function StartTasks() {
  const { screen } = useView();
  return screen === "wizard" || screen === "main" ? <WhatsNewOnStart /> : null;
}

/** Root component: providers, live regions and the active screen. */
export function App() {
  return (
    <PreferencesProvider>
      <ViewProvider>
        <LiveRegions />
        <StartTasks />
        <Screens />
        <TooltipLayer />
      </ViewProvider>
    </PreferencesProvider>
  );
}
