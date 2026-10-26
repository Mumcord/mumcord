import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { stubDialog } from "../channels/testUtils";
import { AppImageMenuPrompt, type AppImageMenuPromptApi } from "./AppImageMenuPrompt";

beforeAll(async () => {
  stubDialog();
  await applyLanguage("en");
});

function fakeApi(pending: boolean) {
  const api: AppImageMenuPromptApi = {
    pending: vi.fn(() => Promise.resolve(pending)),
    answer: vi.fn(() => Promise.resolve()),
  };
  return api;
}

const NAME = "Add Mumcord to the app menu?";

describe("AppImageMenuPrompt", () => {
  it("stays hidden when nothing is to be asked", async () => {
    const api = fakeApi(false);
    render(<AppImageMenuPrompt api={api} />);
    await waitFor(() => {
      expect(api.pending).toHaveBeenCalled();
    });
    expect(screen.queryByRole("dialog", { name: NAME })).toBeNull();
  });

  it("adds the entry on Yes", async () => {
    const api = fakeApi(true);
    render(<AppImageMenuPrompt api={api} />);
    const dialog = await screen.findByRole("dialog", { name: NAME });
    expect(await axeViolations(dialog)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Yes" }));
    await waitFor(() => {
      expect(api.answer).toHaveBeenCalledWith(true);
    });
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: NAME })).toBeNull();
    });
  });

  it("stores No without adding", async () => {
    const api = fakeApi(true);
    render(<AppImageMenuPrompt api={api} />);
    await screen.findByRole("dialog", { name: NAME });
    fireEvent.click(screen.getByRole("button", { name: "No" }));
    await waitFor(() => {
      expect(api.answer).toHaveBeenCalledWith(false);
    });
  });

  it("keeps the question open and says so when saving fails", async () => {
    const api = fakeApi(true);
    vi.mocked(api.answer).mockRejectedValueOnce(new Error("io"));
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    render(<AppImageMenuPrompt api={api} />);
    await screen.findByRole("dialog", { name: NAME });
    fireEvent.click(screen.getByRole("button", { name: "Yes" }));
    expect(await screen.findByRole("alert")).toBeTruthy();
    expect(screen.getByRole("dialog", { name: NAME })).toBeTruthy();
  });
});
