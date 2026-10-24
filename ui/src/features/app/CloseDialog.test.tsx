import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { stubDialog } from "../channels/testUtils";
import { CloseDialog } from "./CloseDialog";

let api: MockApi;

beforeAll(async () => {
  stubDialog();
  await applyLanguage("en");
});

beforeEach(() => {
  api = createMockApi();
  setApi(api);
});

function ask() {
  act(() => {
    api.mock.emit("closeRequested", {});
  });
  return screen.getByRole("dialog", { name: "Minimize to tray or quit?" });
}

describe("CloseDialog", () => {
  it("stays hidden until the engine asks", () => {
    render(<CloseDialog />);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("answers with the choice and remembers it when asked to", async () => {
    const closeChoice = vi.spyOn(api, "closeChoice");
    const { container } = render(<CloseDialog />);
    ask();
    expect(await axeViolations(container)).toEqual([]);
    fireEvent.click(screen.getByRole("checkbox", { name: "Remember my choice" }));
    fireEvent.click(screen.getByRole("button", { name: "Minimize to tray" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
    expect(closeChoice).toHaveBeenCalledWith("minimizeToTray", true);
    expect((await api.settingsGet()).closeBehaviour).toBe("minimizeToTray");
  });

  it("quits without remembering by default", async () => {
    const closeChoice = vi.spyOn(api, "closeChoice");
    render(<CloseDialog />);
    ask();
    fireEvent.click(screen.getByRole("button", { name: "Quit" }));
    await waitFor(() => {
      expect(closeChoice).toHaveBeenCalledWith("quit", false);
    });
    expect((await api.settingsGet()).closeBehaviour).toBe("ask");
  });

  it("Escape keeps the window open without answering", () => {
    const closeChoice = vi.spyOn(api, "closeChoice");
    render(<CloseDialog />);
    const dialog = ask();
    fireEvent(dialog, new Event("cancel", { cancelable: true }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(closeChoice).not.toHaveBeenCalled();
  });
});
