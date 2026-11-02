import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { resetPluginStore } from "../pluginStore";
import { index } from "./fixtures";
import { resetPluginIndex } from "./indexStore";
import { RefreshCatalogueButton } from "./RefreshCatalogueButton";

let api: MockApi;

beforeEach(async () => {
  await applyLanguage("en");
  resetPluginStore();
  resetPluginIndex();
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
});

const press = () => {
  fireEvent.click(screen.getByRole("button", { name: "Refresh catalogue" }));
};

describe("RefreshCatalogueButton", () => {
  it("reloads the lists, checks for updates and reports success", async () => {
    const load = vi.spyOn(api, "pluginsIndex").mockResolvedValue(index([]));
    const check = vi.spyOn(api, "pluginCheckNow").mockResolvedValue(null);
    render(<RefreshCatalogueButton />);
    press();
    expect(screen.getByRole("button", { name: "Refresh catalogue" })).toHaveAttribute(
      "aria-busy",
      "true",
    );
    expect(await screen.findByText("Updated")).toBeInTheDocument();
    expect(load).toHaveBeenCalledTimes(1);
    expect(check).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button")).toHaveAttribute("aria-busy", "false");
  });

  it("says so when only the saved list is available", async () => {
    vi.spyOn(api, "pluginsIndex").mockResolvedValue(index([], { freshness: "offline" }));
    const check = vi.spyOn(api, "pluginCheckNow").mockResolvedValue(null);
    render(<RefreshCatalogueButton />);
    press();
    expect(await screen.findByText("Offline: showing the saved list")).toBeInTheDocument();
    expect(check).not.toHaveBeenCalled();
  });

  it("shows the error text when the list cannot be loaded", async () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    vi.spyOn(api, "pluginsIndex").mockRejectedValue(new Error("boom"));
    render(<RefreshCatalogueButton />);
    press();
    await waitFor(() => {
      expect(screen.getByRole("alert")).toHaveTextContent(/plugin list/i);
    });
  });
});
