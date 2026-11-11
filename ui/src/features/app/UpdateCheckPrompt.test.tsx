import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { stubDialog } from "../channels/testUtils";
import { UpdateCheckPrompt, isPending, type UpdateCheckPromptApi } from "./UpdateCheckPrompt";

beforeAll(async () => {
  stubDialog();
  await applyLanguage("en");
});

function fakeApi(pending: boolean) {
  const api: UpdateCheckPromptApi = {
    pending: vi.fn(() => Promise.resolve(pending)),
    answer: vi.fn(() => Promise.resolve()),
  };
  return api;
}

const NAME = "Check for new versions?";

describe("UpdateCheckPrompt", () => {
  it("stays hidden when the question was answered", async () => {
    const api = fakeApi(false);
    render(<UpdateCheckPrompt api={api} />);
    await waitFor(() => {
      expect(api.pending).toHaveBeenCalled();
    });
    expect(screen.queryByRole("dialog", { name: NAME })).toBeNull();
  });

  it("stays hidden when the package manager owns the install", () => {
    // FreeBSD: `pkg upgrade` is the only update path, so the question never appears.
    const settings = { wizardCompleted: true, updateCheck: "unset" };
    expect(isPending(settings, "appImage")).toBe(true);
    expect(isPending(settings, "pkg")).toBe(false);
    expect(isPending({ ...settings, updateCheck: "no" }, "appImage")).toBe(false);
  });

  it("asks once and stores a yes", async () => {
    const api = fakeApi(true);
    render(<UpdateCheckPrompt api={api} />);
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

  it("stores a no", async () => {
    const api = fakeApi(true);
    render(<UpdateCheckPrompt api={api} />);
    await screen.findByRole("dialog", { name: NAME });
    fireEvent.click(screen.getByRole("button", { name: "No" }));
    await waitFor(() => {
      expect(api.answer).toHaveBeenCalledWith(false);
    });
  });
});
