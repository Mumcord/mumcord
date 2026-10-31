import { fireEvent, render, screen } from "@testing-library/react";
import { useState, type ComponentType } from "react";
import { describe, expect, it } from "vitest";
import { UpdatesCard } from "./AboutSection";
import { ChannelListCard, LookCard } from "./AppearanceSection";
import { ConnectionsCard } from "./PrivacySection";
import { ServerConnectionsCard } from "./ServersSection";
import { createMockApi, setApi, type Settings } from "../../../api";
import { PreferencesProvider } from "../../../app/PreferencesProvider";
import { fakeView } from "../../../app/testView";
import { ViewContext } from "../../../app/view";
import { applyLanguage } from "../../../i18n";
import type { SettingsSectionProps } from "./types";

async function settings() {
  setApi(createMockApi());
  await applyLanguage("en");
  return await (await import("../../../api")).getApi().settingsGet();
}

function AppearanceCards(props: SettingsSectionProps) {
  return (
    <>
      <LookCard {...props} />
      <ChannelListCard {...props} />
    </>
  );
}

/** Renders a section with settings held in state, like the settings screen does. */
function Stateful({
  initial,
  Section,
}: {
  initial: Settings;
  Section: ComponentType<SettingsSectionProps>;
}) {
  const [settings, setSettings] = useState(initial);
  return (
    <ViewContext.Provider value={fakeView()}>
      <Section
        settings={settings}
        update={(patch) => {
          setSettings((previous) => ({ ...previous, ...patch }));
        }}
      />
    </ViewContext.Provider>
  );
}

describe("Epic 2 settings rows", () => {
  it("sets the server limit from 1 to 15 and rejects other input", async () => {
    const current = await settings();
    render(<Stateful initial={current} Section={ServerConnectionsCard} />);
    const limit = screen.getByRole("spinbutton", { name: "Connected servers at once" });
    expect(limit).toHaveValue(5);
    fireEvent.change(limit, { target: { value: "16" } });
    expect(limit).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByRole("alert")).toHaveTextContent("1 to 15");
    fireEvent.change(limit, { target: { value: "12" } });
    expect(limit).toHaveAttribute("aria-invalid", "false");
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("offers the visual effects and channel list switches with their defaults", async () => {
    const current = await settings();
    // The theme editor shows the version the window shows.
    render(
      <PreferencesProvider>
        <Stateful initial={current} Section={AppearanceCards} />
      </PreferencesProvider>,
    );
    expect(screen.getByRole("switch", { name: "Visual effects (glass, grain)" })).toBeChecked();
    const hide = screen.getByRole("switch", { name: "Hide empty channels" });
    expect(hide).not.toBeChecked();
    fireEvent.click(hide);
    expect(hide).toBeChecked();
    expect(screen.getByRole("switch", { name: "Show talking people first" })).not.toBeChecked();
    expect(screen.getByRole("switch", { name: "Show weak connection icon" })).toBeChecked();
  });

  it("asks for direct connections consent in Privacy and for the update check in General", async () => {
    const current = await settings();
    render(<Stateful initial={current} Section={ConnectionsCard} />);
    const direct = screen.getByRole("switch", {
      name: "Allow direct connections to other Mumcord users",
    });
    expect(direct).not.toBeChecked();
    fireEvent.click(direct);
    expect(direct).toBeChecked();
    render(<Stateful initial={current} Section={UpdatesCard} />);
    const check = screen.getByRole("switch", { name: "Check for new versions" });
    fireEvent.click(check);
    expect(check).toBeChecked();
  });

  it("disables the direct-connection consent in modes that never connect directly", async () => {
    const current = await settings();
    render(<Stateful initial={current} Section={ConnectionsCard} />);
    const direct = screen.getByRole("switch", {
      name: "Allow direct connections to other Mumcord users",
    });
    expect(direct).toBeEnabled();
    for (const mode of [/^Relay only/, /^Off/]) {
      fireEvent.click(screen.getByRole("radio", { name: mode }));
      expect(direct).toBeDisabled();
      expect(
        direct
          .closest(".settings-switch")
          ?.querySelector(".info-tip")
          ?.getAttribute("data-tooltip"),
      ).toMatch(/never connects directly/);
    }
    fireEvent.click(screen.getByRole("radio", { name: /^Direct only/ }));
    expect(direct).toBeEnabled();
  });
});
