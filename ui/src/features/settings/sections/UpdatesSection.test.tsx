import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createMockApi,
  setApi,
  type MockApi,
  type Settings,
  type UpdateStatus,
} from "../../../api";
import { applyLanguage } from "../../../i18n";
import { lastChecked } from "./lastChecked";
import { UpdatesSection } from "./UpdatesSection";

let api: MockApi;
let settings: Settings;

beforeEach(async () => {
  await applyLanguage("en");
  api = createMockApi();
  setApi(api);
  settings = { ...(await api.settingsGet()), updateCheck: "yes", updateAutoDownload: false };
});

afterEach(() => {
  setApi(undefined);
});

async function emit(change: Partial<UpdateStatus>) {
  const status = { ...(await api.updateStatus()), ...change };
  act(() => {
    api.mock.emit("updateStatusChanged", { status, prompt: false });
  });
}

describe("UpdatesSection (FR-3.146)", () => {
  it("stores each mode as check and auto-download settings", async () => {
    const update = vi.fn();
    render(<UpdatesSection settings={settings} update={update} />);
    await screen.findByText(/installed from the AppImage/);
    expect(screen.getByRole("radio", { name: "Notify only" })).toBeChecked();
    fireEvent.click(screen.getByRole("radio", { name: "Download automatically" }));
    expect(update).toHaveBeenLastCalledWith({ updateCheck: "yes", updateAutoDownload: true });
    fireEvent.click(screen.getByRole("radio", { name: "Off" }));
    expect(update).toHaveBeenLastCalledWith({ updateCheck: "no", updateAutoDownload: false });
  });

  it("offers no automatic download where the copy cannot install itself", async () => {
    render(
      <UpdatesSection settings={{ ...settings, updateAutoDownload: true }} update={vi.fn()} />,
    );
    await screen.findByText(/installed from/);
    await emit({ install: "deb", canInstall: false });
    const auto = screen.getByRole("radio", { name: "Download automatically" });
    expect(auto).toBeDisabled();
    expect(screen.getByRole("radio", { name: "Notify only" })).toBeChecked();
    expect(screen.getByRole("radiogroup")).toHaveAccessibleDescription(/cannot update itself/);
  });

  it("changes the check interval", async () => {
    const update = vi.fn();
    render(<UpdatesSection settings={settings} update={update} />);
    expect(screen.getByText("On start and every 12 hours")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("combobox", { name: "Check for updates" }));
    fireEvent.click(await screen.findByRole("option", { name: "Once a week" }));
    expect(update).toHaveBeenLastCalledWith({ updateIntervalHours: 168 });
  });

  it("checks now and shows the result", async () => {
    const check = vi.spyOn(api, "updateCheckNow");
    render(<UpdatesSection settings={settings} update={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "Check now" }));
    expect(check).toHaveBeenCalledOnce();
    await emit({ phase: "upToDate", lastCheckMs: Date.now() });
    expect(screen.getByText("Mumcord is up to date")).toBeInTheDocument();
    expect(screen.getByText(/^Last checked today at/)).toBeInTheDocument();
  });

  it("does not check while off or in a development build", async () => {
    const { rerender } = render(
      <UpdatesSection settings={{ ...settings, updateCheck: "no" }} update={vi.fn()} />,
    );
    expect(await screen.findByRole("button", { name: "Check now" })).toBeDisabled();
    rerender(<UpdatesSection settings={settings} update={vi.fn()} />);
    await emit({ devBuild: true });
    expect(screen.getByText("Development build: updates are off")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Check now" })).toBeDisabled();
  });

  it("mentions being offline only quietly", async () => {
    render(<UpdatesSection settings={settings} update={vi.fn()} />);
    await screen.findByText(/installed from/);
    await emit({ phase: "failed", error: "network" });
    expect(screen.getByText(/GitHub could not be reached/)).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("stops skipping a version", async () => {
    const update = vi.fn();
    render(<UpdatesSection settings={{ ...settings, updateSkipped: "0.4.4" }} update={update} />);
    expect(screen.getByText("0.4.4 is skipped and will not be offered again.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Stop skipping" }));
    expect(update).toHaveBeenLastCalledWith({ updateSkipped: null });
    await screen.findByText(/installed from/);
  });
});

describe("lastChecked", () => {
  const t = (key: string, options: Record<string, string>) =>
    `${key} ${Object.values(options).join(" ")}`;
  const noon = new Date(2026, 9, 31, 12, 0).getTime();

  it("says today, yesterday or the date", () => {
    expect(lastChecked(noon - 3_600_000, noon, "en", t)).toMatch(/^updates\.lastCheckedToday /);
    expect(lastChecked(noon - 86_400_000, noon, "en", t)).toMatch(/lastCheckedYesterday/);
    expect(lastChecked(noon - 5 * 86_400_000, noon, "en", t)).toMatch(/lastCheckedOn Oct 26/);
  });
});
