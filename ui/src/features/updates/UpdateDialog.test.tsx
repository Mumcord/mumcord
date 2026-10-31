import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import type { UpdateStatus } from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { stubDialog } from "../channels/testUtils";
import { UpdateDialog, type UpdateActions } from "./UpdateDialog";
import { UpdateToast } from "./UpdateToast";

beforeAll(async () => {
  stubDialog();
  await applyLanguage("en");
});

const URL = "https://github.com/Mumcord/mumcord/releases/tag/v0.4.5";

function statusWith(change: Partial<UpdateStatus>): UpdateStatus {
  return {
    currentVersion: "0.4.2",
    install: "appImage",
    canInstall: true,
    devBuild: false,
    phase: "available",
    offer: {
      version: "0.4.5",
      url: URL,
      notes: [
        {
          version: "0.4.5",
          date: "2026-11-12",
          changes: "### Added\n- Pick the input device\n### Fixed\n- Rare echo",
        },
        { version: "0.4.4", date: "2026-11-03", changes: "### Changed\n- Calmer banner" },
      ],
    },
    progress: null,
    error: null,
    lastCheckMs: null,
    ...change,
  };
}

function fakeActions() {
  return {
    download: vi.fn(() => Promise.resolve()),
    cancel: vi.fn(() => Promise.resolve()),
    restart: vi.fn(() => Promise.resolve()),
    skip: vi.fn<(version: string) => Promise<void>>(() => Promise.resolve()),
    openReleasePage: vi.fn<(url: string) => void>(),
  } satisfies UpdateActions;
}

function renderDialog(
  status: UpdateStatus,
  options: { connected?: number; secondsLeft?: number } = {},
) {
  const actions = fakeActions();
  const onClose = vi.fn();
  const result = render(
    <UpdateDialog
      status={status}
      secondsLeft={options.secondsLeft ?? null}
      connectedServers={options.connected ?? 0}
      actions={actions}
      onClose={onClose}
    />,
  );
  return { ...result, actions, onClose };
}

describe("UpdateDialog: update available", () => {
  it("shows the notes per version with the newest one open", async () => {
    const { container } = renderDialog(statusWith({}));
    const dialog = screen.getByRole("dialog", { name: "Update available" });
    expect(dialog).toHaveTextContent("Mumcord 0.4.5 is ready to download. You have 0.4.2.");
    expect(within(dialog).getByText("Stable")).toBeInTheDocument();
    const newest = screen.getByRole("button", { name: /0\.4\.5/ });
    const older = screen.getByRole("button", { name: /0\.4\.4/ });
    expect(newest).toHaveAttribute("aria-expanded", "true");
    expect(older).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByText("Pick the input device")).toBeInTheDocument();
    expect(screen.getByText("Added")).toBeInTheDocument();
    expect(screen.queryByText("Calmer banner")).not.toBeInTheDocument();
    fireEvent.click(older);
    expect(screen.getByText("Calmer banner")).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("downloads, skips the version or just closes", async () => {
    const { actions, onClose } = renderDialog(statusWith({}));
    fireEvent.click(screen.getByRole("button", { name: "Download and install" }));
    expect(actions.download).toHaveBeenCalledOnce();
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Skip this version" })).toBeEnabled();
    });
    fireEvent.click(screen.getByRole("button", { name: "Remind me later" }));
    expect(onClose).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "Skip this version" }));
    expect(actions.skip).toHaveBeenCalledWith("0.4.5");
    await waitFor(() => {
      expect(onClose).toHaveBeenCalledTimes(2);
    });
  });

  it("says so when an action fails", async () => {
    const { actions } = renderDialog(statusWith({}));
    actions.download.mockRejectedValueOnce(new Error("offline"));
    vi.spyOn(console, "error").mockImplementationOnce(() => undefined);
    fireEvent.click(screen.getByRole("button", { name: "Download and install" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("That did not work");
  });

  it("explains a package install and links the release page instead of downloading", () => {
    const { actions } = renderDialog(statusWith({ install: "deb", canInstall: false }));
    expect(screen.queryByRole("button", { name: "Download and install" })).toBeNull();
    expect(screen.getByText("sudo apt install ./Mumcord_0.4.5_amd64.deb")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Open release page" }));
    expect(actions.openReleasePage).toHaveBeenCalledWith(URL);
  });

  it("asks for the new version by hand where no package manager helps", () => {
    renderDialog(statusWith({ install: "windowsPortable", canInstall: false }));
    expect(screen.getByText("Download the new version from the release page.")).toBeInTheDocument();
  });
});

describe("UpdateDialog: download", () => {
  it("shows the progress, the size and the steps, and cancels", async () => {
    const { actions, container } = renderDialog(
      statusWith({ phase: "downloading", progress: { received: 58_300_000, total: 94_000_000 } }),
      { secondsLeft: 20 },
    );
    expect(screen.getByRole("dialog", { name: "Downloading Mumcord 0.4.5" })).toBeInTheDocument();
    const bar = screen.getByRole("progressbar", { name: "Update progress" });
    expect(bar).toHaveAttribute("aria-valuenow", "62");
    expect(bar).toHaveAttribute("aria-valuemin", "0");
    expect(bar).toHaveAttribute("aria-valuemax", "100");
    expect(screen.getByText(/58 of 94 MB/)).toBeInTheDocument();
    expect(screen.getByText("about 20 seconds left")).toBeInTheDocument();
    const current = container.querySelector("[aria-current='step']");
    expect(current).toHaveTextContent("Download");
    expect(await axeViolations(container)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(actions.cancel).toHaveBeenCalledOnce();
  });

  it("moves on to the verification", () => {
    const { container } = renderDialog(
      statusWith({ phase: "verifying", progress: { received: 94_000_000, total: 94_000_000 } }),
    );
    expect(container.querySelector("[aria-current='step']")).toHaveTextContent(
      "Verify signature and checksum",
    );
  });
});

describe("UpdateDialog: ready", () => {
  it("restarts on request and explains the reconnect while connected", async () => {
    const { actions, onClose, container } = renderDialog(statusWith({ phase: "ready" }), {
      connected: 2,
    });
    expect(screen.getByRole("dialog", { name: "Mumcord 0.4.5 is ready" })).toBeInTheDocument();
    expect(screen.getByText("Verified")).toBeInTheDocument();
    expect(
      screen.getByText(/You are connected to 2 servers\. Mumcord will reconnect/),
    ).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Later" }));
    expect(onClose).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "Restart to update" }));
    expect(actions.restart).toHaveBeenCalledOnce();
  });

  it("says the Windows installer runs with the restart", () => {
    renderDialog(statusWith({ phase: "ready", install: "windowsInstaller" }));
    expect(screen.getByText("Installs when Mumcord restarts")).toBeInTheDocument();
    expect(screen.queryByText(/You are connected/)).toBeNull();
  });
});

describe("UpdateDialog: failed", () => {
  it("says a bad download was discarded and retries", () => {
    const { actions, onClose } = renderDialog(statusWith({ phase: "failed", error: "signature" }));
    expect(screen.getByRole("alert")).toHaveTextContent("the download was discarded");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(actions.download).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(onClose).toHaveBeenCalledOnce();
  });
});

describe("UpdateToast", () => {
  function renderToast(connected: number) {
    const props = {
      version: "0.4.5",
      connectedServers: connected,
      onRestart: vi.fn(),
      onShowDialog: vi.fn(),
      onLater: vi.fn(),
    };
    render(<UpdateToast {...props} />);
    return props;
  }

  it("restarts right away when no server is connected", () => {
    const props = renderToast(0);
    expect(screen.getByRole("region", { name: "Mumcord 0.4.5 is ready" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Restart" }));
    expect(props.onRestart).toHaveBeenCalledOnce();
  });

  it("asks first during a call instead of restarting", () => {
    const props = renderToast(1);
    fireEvent.click(screen.getByRole("button", { name: "Restart" }));
    expect(props.onRestart).not.toHaveBeenCalled();
    expect(props.onShowDialog).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "Later" }));
    expect(props.onLater).toHaveBeenCalledOnce();
  });
});
