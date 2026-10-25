import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type Settings } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { ChatHistorySection } from "./ChatHistorySection";

let settings: Settings;

beforeEach(async () => {
  await applyLanguage("en");
  const api = createMockApi();
  setApi(api);
  settings = await api.settingsGet();
});

afterEach(() => {
  setApi(undefined);
});

describe("ChatHistorySection: history sharing (FR-1.60)", () => {
  it("is on by default and turns sharing off", async () => {
    const update = vi.fn();
    render(<ChatHistorySection settings={settings} update={update} />);
    const toggle = screen.getByRole("switch", { name: "Share my channel history" });
    expect(toggle).toBeChecked();
    expect(toggle).toHaveAccessibleDescription(/Private chats are never shared/);
    fireEvent.click(toggle);
    expect(update).toHaveBeenCalledWith({ historySharing: false });
    await screen.findByText("Servers");
  });

  it("turns sharing back on", async () => {
    const update = vi.fn();
    render(
      <ChatHistorySection settings={{ ...settings, historySharing: false }} update={update} />,
    );
    const toggle = screen.getByRole("switch", { name: "Share my channel history" });
    expect(toggle).not.toBeChecked();
    fireEvent.click(toggle);
    expect(update).toHaveBeenCalledWith({ historySharing: true });
    await screen.findByText("Servers");
  });
});
