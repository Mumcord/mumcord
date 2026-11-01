import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../api";
import { PreferencesContext, type Preferences } from "../../app/preferences";
import { fakeView } from "../../app/testView";
import { ViewContext } from "../../app/view";
import { applyLanguage } from "../../i18n";
import { SettingsScreen } from "./SettingsScreen";

// Changed badges, sidebar dots, Reset section, the active preset card, the Beta features card
// and the privacy summary (#161).

let api: MockApi;

beforeEach(async () => {
  await applyLanguage("en");
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  setApi(undefined);
});

function renderScreen() {
  const preferences: Preferences = {
    theme: "system",
    resolvedTheme: "dark",
    language: "en",
    chatDisplay: "cozy",
    linkPreviews: true,
    hideEmptyChannels: false,
    talkingFirst: false,
    showWeakConnectionIcon: true,
    lowLatency: false,
    applySettings: vi.fn<Preferences["applySettings"]>(),
  };
  render(
    <PreferencesContext.Provider value={preferences}>
      <ViewContext.Provider value={fakeView()}>
        <SettingsScreen />
      </ViewContext.Provider>
    </PreferencesContext.Provider>,
  );
}

async function openSection(name: string) {
  fireEvent.click(await screen.findByRole("tab", { name }));
  // The preset card is ready once the defaults, overlay and Do Not Disturb have loaded.
  await waitFor(() => {
    expect(screen.getByRole("button", { name: "Switch preset" })).toBeEnabled();
  });
}

describe("Changed badges and Reset section", () => {
  it("marks a changed switch and its group, and resets the group after a confirm", async () => {
    renderScreen();
    await openSection("General");
    expect(screen.getByRole("tab", { name: "General" })).not.toHaveAccessibleDescription();

    fireEvent.click(screen.getByRole("switch", { name: "Skip links" }));
    const changed = await screen.findByRole("switch", { name: "Skip links Changed" });
    expect(changed).not.toBeChecked();
    expect(screen.getByRole("tab", { name: "General" })).toHaveAccessibleDescription(
      "Has changed settings",
    );

    fireEvent.click(screen.getByRole("button", { name: "Reset section" }));
    const dialog = screen.getByRole("dialog", { name: "Reset General?" });
    expect(
      within(dialog)
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual(["Skip links"]);
    fireEvent.click(within(dialog).getByRole("button", { name: "Reset" }));

    expect(await screen.findByRole("switch", { name: "Skip links" })).toBeChecked();
    await waitFor(async () => {
      expect((await api.settingsGet()).skipLinks).toBe(true);
    });
    expect(screen.getByRole("tab", { name: "General" })).not.toHaveAccessibleDescription();
  });

  it("says so when there is nothing to reset and changes nothing", async () => {
    renderScreen();
    await openSection("Appearance");
    fireEvent.click(screen.getByRole("button", { name: "Reset section" }));
    const dialog = screen.getByRole("dialog", { name: "Reset Appearance?" });
    expect(dialog).toHaveTextContent(
      "Every setting in Appearance already has its preset or default value.",
    );
    expect(within(dialog).queryByRole("button", { name: "Reset" })).toBeNull();
    fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("shows Recommended next to the recommended choices", async () => {
    renderScreen();
    await openSection("Voice & Audio");
    expect(screen.getByRole("radio", { name: /^Voice activation.* Recommended$/ })).toBeChecked();
    expect(screen.getByRole("radio", { name: "Standard Recommended" })).toBeInTheDocument();
  });
});

describe("Active preset card", () => {
  it("switches to a preset after listing what changes, then counts later changes", async () => {
    renderScreen();
    await openSection("Account & Identity");
    const card = screen.getByRole("region", { name: "Active preset" });
    expect(card).toHaveTextContent("No preset");
    expect(card).toHaveTextContent("No changes since setup");

    fireEvent.click(within(card).getByRole("button", { name: "Switch preset" }));
    const picker = screen.getByRole("dialog", { name: "Switch preset" });
    expect(within(picker).getAllByRole("radio")).toHaveLength(4);
    fireEvent.click(within(picker).getByRole("radio", { name: "Everything on" }));
    fireEvent.click(within(picker).getByRole("button", { name: "Continue" }));

    const confirm = screen.getByRole("dialog", { name: "Switch to Everything on?" });
    const listed = within(confirm)
      .getAllByRole("listitem")
      .map((item) => item.textContent);
    expect(listed).toContain("Share what I'm listening to");
    fireEvent.click(within(confirm).getByRole("button", { name: "Switch preset" }));

    await waitFor(async () => {
      const saved = await api.settingsGet();
      expect(saved.setupPreset).toBe("everythingOn");
      expect(saved.nowPlaying).toBe(true);
      expect(saved.privateMessages.rule).toBe("everyone");
    });
    await waitFor(() => {
      expect(card).toHaveTextContent("Everything on");
    });
    expect(card).toHaveTextContent("No changes since setup");
    // Compared with the preset now, so the preset's own value is not "changed".
    expect(screen.getByRole("switch", { name: "Share what I'm listening to" })).toBeChecked();

    fireEvent.click(screen.getByRole("switch", { name: "Share what I'm listening to" }));
    await waitFor(() => {
      expect(card).toHaveTextContent("Everything on, customised");
    });
    expect(card).toHaveTextContent("1 setting changed since setup");
  });

  it("goes back from the confirm step without changing anything", async () => {
    renderScreen();
    await openSection("General");
    fireEvent.click(screen.getByRole("button", { name: "Switch preset" }));
    const dialog = screen.getByRole("dialog", { name: "Switch preset" });
    fireEvent.click(within(dialog).getByRole("radio", { name: "Custom" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Continue" }));
    expect(screen.getByRole("dialog", { name: "Switch to Custom?" })).toHaveTextContent(
      "No setting changes.",
    );
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect((await api.settingsGet()).setupPreset).toBeNull();
  });
});

describe("Beta features card", () => {
  it("holds the Beta switches, bound to the same settings as in context", async () => {
    renderScreen();
    await openSection("Advanced & Developer");
    const card = screen.getByRole("region", { name: "Beta features" });
    const positional = within(card).getByRole("switch", { name: "Positional audio Beta feature" });
    expect(within(card).getAllByRole("switch")).toHaveLength(4);
    fireEvent.click(positional);
    await waitFor(async () => {
      expect((await api.settingsGet()).positional.enabled).toBe(true);
    });
    expect(
      await within(card).findByRole("switch", { name: "Positional audio Beta feature Changed" }),
    ).toBeChecked();

    fireEvent.click(within(card).getByRole("button", { name: "Manage plugins" }));
    expect(await screen.findByRole("tab", { name: "Gaming & Overlay" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(
      await screen.findByRole("switch", { name: "Positional audio Beta feature Changed" }),
    ).toBeChecked();
  });
});

describe("Privacy summary", () => {
  it("sums up the privacy settings and links to the cards", async () => {
    renderScreen();
    await openSection("Privacy & Connections");
    const card = screen.getByRole("region", { name: /^Privacy summary/ });
    expect(card).toHaveTextContent("Direct connections can show your IP address");
    expect(card).toHaveTextContent("History sync is");
    fireEvent.click(within(card).getByRole("button", { name: "Change: Private messages" }));
    expect(await screen.findByRole("tab", { name: "Chat & Notifications" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });
});
