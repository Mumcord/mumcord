import { act, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createMockApi, setApi, type MockApi, type PluginInstallStage } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { InstallProgress } from "./InstallProgress";
import { clearInstallProgress } from "./installProgressStore";

let api: MockApi;

beforeEach(async () => {
  await applyLanguage("en");
  api = createMockApi();
  setApi(api);
});

function emit(id: string, progress: PluginInstallStage) {
  act(() => {
    api.mock.emit("pluginsInstallProgress", { id, progress });
  });
}

const downloading = (received: number): PluginInstallStage => ({
  stage: "downloading",
  received,
  total: 23_000_000,
});

describe("InstallProgress", () => {
  it("renders nothing while no install runs", () => {
    const { container } = render(<InstallProgress id="eve" name="EVE Fit" />);
    expect(container.innerHTML).toBe("");
  });

  it("shows the downloaded bytes, then each step, and disappears when done", async () => {
    const { container } = render(<InstallProgress id="eve" name="EVE Fit" />);
    emit("eve", downloading(5_750_000));
    const bar = screen.getByRole("progressbar", { name: "Installing EVE Fit" });
    expect(bar.getAttribute("aria-valuenow")).toBe("25");
    expect(bar.getAttribute("aria-valuemin")).toBe("0");
    expect(bar.getAttribute("aria-valuemax")).toBe("100");
    expect(screen.getByText("Downloading 5.8 MB of 23 MB")).toBeTruthy();
    expect(await axeViolations(container)).toEqual([]);

    emit("eve", { stage: "verifying" });
    expect(bar.getAttribute("aria-valuenow")).toBe("100");
    expect(screen.getByText("Checking signature…")).toBeTruthy();
    emit("eve", { stage: "unpacking" });
    expect(screen.getByText("Unpacking…")).toBeTruthy();
    emit("eve", { stage: "starting" });
    expect(screen.getByText("Starting…")).toBeTruthy();
    emit("eve", { stage: "done" });
    expect(screen.queryByRole("progressbar")).toBeNull();
  });

  it("announces the download only in quarter steps", () => {
    render(<InstallProgress id="eve" name="EVE Fit" />);
    const status = () => screen.getByRole("status").textContent;
    emit("eve", downloading(230_000));
    expect(status()).toBe("EVE Fit: 0% downloaded");
    emit("eve", downloading(5_000_000));
    expect(status()).toBe("EVE Fit: 0% downloaded");
    emit("eve", downloading(12_000_000));
    expect(status()).toBe("EVE Fit: 50% downloaded");
    emit("eve", { stage: "verifying" });
    expect(status()).toBe("EVE Fit: Checking signature…");
  });

  it("ignores other plugins, ends on failure and can be cleared", () => {
    render(<InstallProgress id="eve" name="EVE Fit" />);
    emit("dice", downloading(1_000));
    expect(screen.queryByRole("progressbar")).toBeNull();
    emit("eve", downloading(1_000));
    emit("eve", { stage: "failed", reason: "Download failed" });
    expect(screen.queryByRole("progressbar")).toBeNull();
    emit("eve", { stage: "verifying" });
    act(() => {
      clearInstallProgress("eve");
    });
    expect(screen.queryByRole("progressbar")).toBeNull();
  });

  it("shows a slim bar in list rows", () => {
    const { container } = render(<InstallProgress id="eve" name="EVE Fit" variant="slim" />);
    emit("eve", downloading(23_000_000));
    expect(container.querySelector(".plugin-progress--slim")).not.toBeNull();
    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("100");
  });
});
