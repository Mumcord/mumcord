import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { stubDialog } from "../channels/testUtils";
import { AudioDeviceNotice, type AudioDeviceNoticeApi } from "./AudioDeviceNotice";

beforeAll(async () => {
  stubDialog();
  await applyLanguage("en");
});

function fakeApi(pending: boolean) {
  const api: AudioDeviceNoticeApi = {
    pending: vi.fn(() => Promise.resolve(pending)),
    dismiss: vi.fn(() => Promise.resolve()),
  };
  return api;
}

const NAME = "Choose your audio devices again";

describe("AudioDeviceNotice", () => {
  it("stays hidden when there is nothing to announce", async () => {
    const api = fakeApi(false);
    render(<AudioDeviceNotice api={api} onOpenSettings={vi.fn()} />);
    await waitFor(() => {
      expect(api.pending).toHaveBeenCalled();
    });
    expect(screen.queryByRole("dialog", { name: NAME })).toBeNull();
  });

  it("is shown once and marked as shown when dismissed", async () => {
    const api = fakeApi(true);
    const open = vi.fn();
    const { rerender } = render(<AudioDeviceNotice api={api} onOpenSettings={open} />);
    const dialog = await screen.findByRole("dialog", { name: NAME });
    expect(await axeViolations(dialog)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Got it" }));
    expect(api.dismiss).toHaveBeenCalledTimes(1);
    expect(open).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog", { name: NAME })).toBeNull();
    rerender(<AudioDeviceNotice api={api} onOpenSettings={open} />);
    expect(api.dismiss).toHaveBeenCalledTimes(1);
  });

  it("opens the audio settings and marks the notice as shown", async () => {
    const api = fakeApi(true);
    const open = vi.fn();
    render(<AudioDeviceNotice api={api} onOpenSettings={open} />);
    fireEvent.click(await screen.findByRole("button", { name: "Open audio settings" }));
    expect(api.dismiss).toHaveBeenCalledTimes(1);
    expect(open).toHaveBeenCalledTimes(1);
  });
});
