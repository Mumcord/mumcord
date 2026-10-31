import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createMockApi, setApi, type MockApi, type UpdateStatus } from "../../api";
import { applyLanguage } from "../../i18n";
import { stubDialog } from "../channels/testUtils";
import { Updater } from "./Updater";

let api: MockApi;

beforeAll(async () => {
  stubDialog();
  await applyLanguage("en");
});

beforeEach(() => {
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  setApi(undefined);
});

async function emit(change: Partial<UpdateStatus>, prompt: boolean) {
  const status = { ...(await api.updateStatus()), ...change };
  act(() => {
    api.mock.emit("updateStatusChanged", { status, prompt });
  });
}

const offer = { version: "0.4.5", url: "https://example.com", notes: [] };

describe("Updater", () => {
  it("opens the dialog when the engine asks for it", async () => {
    render(<Updater />);
    await emit({ phase: "available", offer }, false);
    expect(screen.queryByRole("dialog")).toBeNull();
    await emit({ phase: "available", offer }, true);
    expect(screen.getByRole("dialog", { name: "Update available" })).toBeInTheDocument();
  });

  it("shows the toast once the ready dialog is closed, until Later", async () => {
    render(<Updater />);
    await emit({ phase: "ready", offer }, true);
    fireEvent.click(screen.getByRole("button", { name: "Later" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("region", { name: "Mumcord 0.4.5 is ready" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Later" }));
    expect(screen.queryByRole("region", { name: "Mumcord 0.4.5 is ready" })).toBeNull();
  });
});
