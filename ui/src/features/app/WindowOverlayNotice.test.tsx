import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { stubDialog } from "../channels/testUtils";
import { WindowOverlayNotice, type WindowOverlayNoticeApi } from "./WindowOverlayNotice";

beforeAll(async () => {
  stubDialog();
  await applyLanguage("en");
});

function fakeApi(pending: boolean) {
  const api: WindowOverlayNoticeApi = {
    pending: vi.fn(() => Promise.resolve(pending)),
    dismiss: vi.fn(() => Promise.resolve()),
  };
  return api;
}

const NAME = "The window overlay was removed";

describe("WindowOverlayNotice", () => {
  it("stays hidden when there is nothing to announce", async () => {
    const api = fakeApi(false);
    render(<WindowOverlayNotice api={api} onOpenSettings={vi.fn()} />);
    await waitFor(() => {
      expect(api.pending).toHaveBeenCalled();
    });
    expect(screen.queryByRole("dialog", { name: NAME })).toBeNull();
  });

  it("is shown once and marked as shown when dismissed", async () => {
    const api = fakeApi(true);
    const open = vi.fn();
    const { rerender } = render(<WindowOverlayNotice api={api} onOpenSettings={open} />);
    const dialog = await screen.findByRole("dialog", { name: NAME });
    expect(await axeViolations(dialog)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Got it" }));
    expect(api.dismiss).toHaveBeenCalledTimes(1);
    expect(open).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog", { name: NAME })).toBeNull();
    rerender(<WindowOverlayNotice api={api} onOpenSettings={open} />);
    expect(api.dismiss).toHaveBeenCalledTimes(1);
  });

  it("opens the overlay settings and marks the notice as shown", async () => {
    const api = fakeApi(true);
    const open = vi.fn();
    render(<WindowOverlayNotice api={api} onOpenSettings={open} />);
    fireEvent.click(await screen.findByRole("button", { name: "Open overlay settings" }));
    expect(api.dismiss).toHaveBeenCalledTimes(1);
    expect(open).toHaveBeenCalledTimes(1);
  });
});
