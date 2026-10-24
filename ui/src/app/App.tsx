import { useTranslation } from "react-i18next";
import { LiveRegions } from "../a11y";
import { AppShell } from "./AppShell";
import {
  ChannelsMount,
  ChatMount,
  ChatStatusMount,
  MembersMount,
  ServersMount,
  UserPanelMount,
  WizardMount,
} from "./mounts";
import { PreferencesProvider } from "./PreferencesProvider";
import { SettingsOverlay } from "./SettingsOverlay";
import { useView } from "./view";
import { ViewProvider } from "./ViewProvider";

function MainScreen() {
  const { overlay, closeOverlay } = useView();
  return (
    <>
      <AppShell
        inert={overlay !== null}
        slots={{
          servers: <ServersMount />,
          channels: <ChannelsMount />,
          userPanel: <UserPanelMount />,
          chatStatus: <ChatStatusMount />,
          chat: <ChatMount />,
          members: <MembersMount />,
        }}
      />
      {overlay === "settings" && <SettingsOverlay onClose={closeOverlay} />}
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

/** Root component: providers, live regions and the active screen. */
export function App() {
  return (
    <PreferencesProvider>
      <ViewProvider>
        <LiveRegions />
        <Screens />
      </ViewProvider>
    </PreferencesProvider>
  );
}
