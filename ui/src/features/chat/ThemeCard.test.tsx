import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createMockApi, setApi, type ChatMessage, type MockApi } from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { MUMCORD_PRESET } from "../../theme/themeModel";
import { formatShareMessage } from "../../theme/shareCodec";
import { PreferencesProvider } from "../../app/PreferencesProvider";
import { MessageList } from "./MessageList";

const THEME = {
  name: "Ocean <b>deep</b>",
  dark: { ...MUMCORD_PRESET.dark, accent: "#1e90ff" },
  light: MUMCORD_PRESET.light,
};

function message(body: string): ChatMessage {
  return {
    id: "1",
    conversation: { kind: "channel", channelId: 1 },
    senderName: "Maya",
    senderCertHash: null,
    senderSession: null,
    own: false,
    sentAt: new Date().toISOString(),
    body,
    dismissedPreviews: [],
    syncedFrom: null,
  };
}

function renderMessage(body: string) {
  return render(
    <PreferencesProvider>
      <main>
        <MessageList
          log={{
            messages: [message(body)],
            loaded: true,
            loading: false,
            hasMore: false,
            loadFailed: false,
            syncing: false,
            syncResult: null,
            syncFailed: false,
          }}
          label="Messages in #Lobby"
          onLoadOlder={() => undefined}
          linkPreviews={false}
        />
      </main>
    </PreferencesProvider>,
  );
}

let api: MockApi;

beforeAll(async () => {
  await applyLanguage("en");
});

beforeEach(() => {
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  setApi(undefined);
});

describe("theme card", () => {
  it("shows the name as plain text with a mock-up and apply button", async () => {
    const { container } = renderMessage(formatShareMessage(THEME, "0.1.0"));
    const card = await screen.findByRole("region", { name: "Shared theme Ocean <b>deep</b>" });
    expect(card).toHaveTextContent("Ocean <b>deep</b>");
    expect(card.querySelector("b")).toBeNull();
    expect(card.querySelector(".theme-card__mock")).not.toBeNull();
    expect(screen.queryByText("Imported ✓")).toBeNull();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("shows the mock-up in the version the window shows", async () => {
    const { container } = renderMessage(formatShareMessage(THEME, "0.1.0"));
    await screen.findByRole("region", { name: /Shared theme/ });
    const mock = container.querySelector<HTMLElement>(".theme-card__mock");
    // jsdom has no OS colour scheme, so Follow system shows the dark version.
    expect(mock?.style.getPropertyValue("--mock-accent")).toBe("#1e90ff");
  });

  it("shows a card for a code from older builds with one colour set", async () => {
    const json = JSON.stringify({ v: "0.3.0", n: "Old", colors: THEME.dark });
    const code = `mct1.${btoa(json).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "")}`;
    renderMessage(`🎨 Mumcord theme "Old" – ${code}`);
    expect(await screen.findByRole("region", { name: "Shared theme Old" })).toBeInTheDocument();
  });

  it("hides the raw share text when the card is shown", async () => {
    const share = formatShareMessage(THEME, "0.1.0");
    const { container } = renderMessage(share);
    await screen.findByRole("region", { name: /Shared theme/ });
    expect(container.textContent).not.toContain("mct1.");
    expect(container.textContent).not.toContain('Mumcord theme "');
  });

  it("renders no empty body for a message that is only the share line", async () => {
    const { container } = renderMessage(formatShareMessage(THEME, "0.1.0"));
    await screen.findByRole("region", { name: /Shared theme/ });
    expect(container.querySelector(".chat-message__body")).toBeNull();
  });

  it("keeps other text and drops only the share line", async () => {
    const share = formatShareMessage(THEME, "0.1.0");
    const { container } = renderMessage(`Check this out<br>${share}<br>Cool, right?`);
    await screen.findByRole("region", { name: /Shared theme/ });
    const body = container.querySelector(".chat-message__body");
    expect(body?.textContent).toContain("Check this out");
    expect(body?.textContent).toContain("Cool, right?");
    expect(container.textContent).not.toContain("mct1.");
  });

  it("shows the text as is when the code is invalid", () => {
    const { container } = renderMessage("hello mct1.not-a-theme");
    expect(container.textContent).toContain("mct1.not-a-theme");
  });

  it("shows no card for a message without a valid code", () => {
    renderMessage("hello mct1.not-a-theme");
    expect(screen.queryByRole("region", { name: /Shared theme/ })).toBeNull();
    renderMessage("hello");
    expect(screen.queryByRole("region", { name: /Shared theme/ })).toBeNull();
  });

  it("applies as app theme once, then shows Imported", async () => {
    renderMessage(formatShareMessage(THEME, "0.1.0"));
    fireEvent.click(await screen.findByRole("button", { name: "Apply theme" }));
    expect(await screen.findByText("Imported ✓")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Apply theme" }));
    await waitFor(async () => {
      const settings = await api.settingsGet();
      expect(settings.customThemes).toHaveLength(1);
      expect(settings.appThemeId).toBe(settings.customThemes[0]?.id);
    });
  });

  it("applies to a server from the arrow menu without a duplicate", async () => {
    const saved = await api.serversList();
    renderMessage(formatShareMessage(THEME, "0.1.0"));
    const arrow = await screen.findByRole("button", { name: "Apply to a server" });
    await waitFor(() => {
      expect(arrow).toBeEnabled();
    });
    fireEvent.click(arrow);
    const items = await screen.findAllByRole("menuitem");
    expect(items).toHaveLength(saved.length);
    fireEvent.click(items[0] as HTMLElement);
    await waitFor(async () => {
      const settings = await api.settingsGet();
      expect(settings.customThemes).toHaveLength(1);
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply theme" }));
    await screen.findByText("Imported ✓");
    expect((await api.settingsGet()).customThemes).toHaveLength(1);
  });

  it("notes a theme made with a newer Mumcord", async () => {
    renderMessage(formatShareMessage(THEME, "99.0.0"));
    expect(await screen.findByText(/Made with Mumcord 99\.0\.0/)).toBeInTheDocument();
  });
});
