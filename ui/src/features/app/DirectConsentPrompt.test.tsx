import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { stubDialog } from "../channels/testUtils";
import { DirectConsentPrompt, type DirectConsentPromptApi } from "./DirectConsentPrompt";

beforeAll(async () => {
  stubDialog();
  await applyLanguage("en");
});

function fakeApi(answer: () => Promise<void> = () => Promise.resolve()) {
  let required: (() => void) | null = null;
  const api: DirectConsentPromptApi = {
    onRequired: vi.fn((handler: () => void) => {
      required = handler;
      return () => {
        required = null;
      };
    }),
    answer: vi.fn(answer),
  };
  const ask = () => {
    act(() => {
      required?.();
    });
  };
  return { api, ask };
}

const NAME = "Allow direct connections?";

describe("DirectConsentPrompt", () => {
  it("stays hidden until a direct connection would be used", () => {
    const { api } = fakeApi();
    render(<DirectConsentPrompt api={api} />);
    expect(api.onRequired).toHaveBeenCalled();
    expect(screen.queryByRole("dialog", { name: NAME })).toBeNull();
  });

  it("explains the IP address and stores a yes", async () => {
    const { api, ask } = fakeApi();
    render(<DirectConsentPrompt api={api} />);
    ask();
    const dialog = await screen.findByRole("dialog", { name: NAME });
    expect(dialog).toHaveAccessibleDescription(/the other person can see your IP address/);
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
    const { api, ask } = fakeApi();
    render(<DirectConsentPrompt api={api} />);
    ask();
    await screen.findByRole("dialog", { name: NAME });
    fireEvent.click(screen.getByRole("button", { name: "No" }));
    await waitFor(() => {
      expect(api.answer).toHaveBeenCalledWith(false);
    });
  });

  it("needs an explicit answer: Escape keeps it open", async () => {
    const { api, ask } = fakeApi();
    render(<DirectConsentPrompt api={api} />);
    ask();
    const dialog = await screen.findByRole("dialog", { name: NAME });
    const cancel = new Event("cancel", { cancelable: true });
    dialog.dispatchEvent(cancel);
    expect(cancel.defaultPrevented).toBe(true);
    expect(screen.getByRole("dialog", { name: NAME })).toBeInTheDocument();
  });

  it("keeps the question open when saving fails", async () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    const { api, ask } = fakeApi(() => Promise.reject(new Error("store")));
    render(<DirectConsentPrompt api={api} />);
    ask();
    await screen.findByRole("dialog", { name: NAME });
    fireEvent.click(screen.getByRole("button", { name: "Yes" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("That did not work");
    expect(screen.getByRole("dialog", { name: NAME })).toBeInTheDocument();
  });
});
