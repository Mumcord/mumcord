import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { FileSharingSettings } from "./FileSharingSettings";
import { createMockApi, setApi, type Settings } from "../../api";
import { applyLanguage } from "../../i18n";

const pick = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: pick }));

async function renderCard() {
  setApi(createMockApi());
  await applyLanguage("en");
  const base = await (await import("../../api")).getApi().settingsGet();
  const update = vi.fn<(patch: Partial<Settings>) => void>();
  render(<FileSharingSettings settings={base} update={update} />);
  return update;
}

describe("FileSharingSettings", () => {
  it("toggles receiving and shows the notes without a malware line", async () => {
    const update = await renderCard();
    fireEvent.click(screen.getByRole("switch", { name: /Receive files from other Mumcord users/ }));
    expect(update).toHaveBeenCalledWith({ filesReceive: false });
    expect(screen.getByText(/end-to-end encrypted/)).toBeInTheDocument();
    expect(screen.queryByText(/malware/i)).toBeNull();
  });

  it("offers the four size limits", async () => {
    const update = await renderCard();
    fireEvent.click(screen.getByRole("combobox", { name: "Maximum file size" }));
    for (const name of ["100 MB", "500 MB", "1 GB", "2 GB"]) {
      expect(screen.getByRole("option", { name })).toBeInTheDocument();
    }
    fireEvent.click(screen.getByRole("option", { name: "2 GB" }));
    expect(update).toHaveBeenCalledWith({ filesMaxSize: "2G" });
  });

  it("writes the folder picked with the dialog", async () => {
    pick.mockResolvedValueOnce("/data/in");
    const update = await renderCard();
    await waitFor(() => {
      expect(screen.getByLabelText("Save files to")).toHaveValue("~/Downloads");
    });
    fireEvent.click(screen.getByRole("button", { name: "Change…" }));
    await waitFor(() => {
      expect(update).toHaveBeenCalledWith({ filesSaveFolder: "/data/in" });
    });
  });
});
