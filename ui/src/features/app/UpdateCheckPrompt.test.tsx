import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { stubDialog } from "../channels/testUtils";
import { UpdateCheckPrompt, type UpdateCheckPromptApi } from "./UpdateCheckPrompt";

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
