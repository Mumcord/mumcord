import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createMockApi,
  EngineError,
  setApi,
  type MockApi,
  type PluginInstallOffer,
} from "../../../api";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { indexed } from "../browse/fixtures";
import { InstallPrompt } from "./InstallPrompt";
import { resetPluginToasts } from "./installToast";
import { PluginToasts } from "./PluginToasts";

let api: MockApi;

const poll = indexed("poll", { permissions: ["ui.chat-card", "peers.messages", "ui.panel"] });
const offer: PluginInstallOffer = {
  id: "poll",
  version: "1.1.0",
  authors: ["Mumcord"],
  required: ["ui.chat-card", "peers.messages"],
  optional: [
    { id: "ui.panel", suggested: true },
    { id: "notify", suggested: false },
  ],
  defaultVisibility: "friends",
  chatPatterns: [],
};

beforeEach(async () => {
  await applyLanguage("en");
  resetPluginToasts();
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
});

describe("InstallPrompt boxes", () => {
  it("says who suggested the install and what an official plugin never gets", async () => {
    vi.spyOn(api, "pluginInstallPrepare").mockResolvedValue(offer);
    render(
      <InstallPrompt
        plugin={poll}
        repositoryWarning={false}
        suggestedBy={{ serverId: 99, reason: "We use it for raids" }}
        onClose={vi.fn()}
      />,
    );
    expect(await screen.findByText("Suggested by this server")).toBeTruthy();
    expect(screen.getByText("We use it for raids")).toBeTruthy();
    expect(screen.getByText("It never gets")).toBeTruthy();
  });

  it("shows neither box for a plain repository install", async () => {
    vi.spyOn(api, "pluginInstallPrepare").mockResolvedValue(offer);
    const repo = indexed("poll", { source: { kind: "repo", repositoryId: 1, name: "Fans" } });
    render(<InstallPrompt plugin={repo} repositoryWarning={false} onClose={vi.fn()} />);
    await screen.findAllByText("Required");
    expect(screen.queryByText("It never gets")).toBeNull();
    expect(screen.queryByText(/^Suggested by/)).toBeNull();
  });
});

describe("InstallPrompt", () => {
  it("shows what the plugin's chat patterns recognise, as plain text", async () => {
    vi.spyOn(api, "pluginInstallPrepare").mockResolvedValue({
      ...offer,
      required: ["chat.patterns"],
      optional: [],
      chatPatterns: ["Recognise EVE Online fits pasted in chat", "<b>not markup</b>"],
    });
    render(<InstallPrompt plugin={poll} repositoryWarning={false} onClose={vi.fn()} />);
    expect(
      await screen.findByText("Recognise certain chat messages, such as pasted game fits"),
    ).toBeTruthy();
    expect(screen.getByText("Recognise EVE Online fits pasted in chat")).toBeTruthy();
    expect(screen.getByText("<b>not markup</b>")).toBeTruthy();
  });

  it("shows required and optional permissions and the visibility choice, then installs", async () => {
    vi.spyOn(api, "pluginInstallPrepare").mockResolvedValue(offer);
    const install = vi.spyOn(api, "pluginInstall").mockResolvedValue(null);
    const onClose = vi.fn();
    render(
      <>
        <InstallPrompt plugin={poll} repositoryWarning={false} onClose={onClose} />
        <PluginToasts />
      </>,
    );
    const dialog = await screen.findByRole("dialog", { name: "Install Poll?" });
    expect(await screen.findByText("Official · 1.1.0")).toBeTruthy();
    expect(screen.getAllByText("Required")).toHaveLength(2);
    expect(screen.getByText("Change later in Settings → Plugins.")).toBeTruthy();
    expect(screen.getByRole("radio", { name: "Friends" })).toHaveProperty("checked", true);
    expect(await axeViolations(dialog)).toEqual([]);

    fireEvent.click(screen.getByRole("radio", { name: "Everyone" }));
    fireEvent.click(screen.getByRole("button", { name: "Install" }));
    await waitFor(() => {
      expect(onClose).toHaveBeenCalled();
    });
    expect(install).toHaveBeenCalledWith("poll", "1.1.0", ["ui.panel"], "everyone");
    expect(screen.getByText("Poll installed")).toBeTruthy();
  });

  it("hides the visibility choice without peers.messages", async () => {
    vi.spyOn(api, "pluginInstallPrepare").mockResolvedValue({
      ...offer,
      required: ["ui.chat-card"],
      optional: [],
    });
    render(<InstallPrompt plugin={poll} repositoryWarning={false} onClose={vi.fn()} />);
    await screen.findByText("Official · 1.1.0");
    expect(screen.queryByRole("radio")).toBeNull();
  });

  it("shows the install stage and a verification error", async () => {
    vi.spyOn(api, "pluginInstallPrepare").mockResolvedValue(offer);
    let fail: (error: unknown) => void = () => {};
    vi.spyOn(api, "pluginInstall").mockReturnValue(
      new Promise((_, reject) => {
        fail = reject;
      }),
    );
    render(<InstallPrompt plugin={poll} repositoryWarning={false} onClose={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "Install" }));
    act(() => {
      api.mock.emit("pluginsInstallProgress", { id: "poll", progress: { stage: "verifying" } });
    });
    expect(await screen.findByText("Checking…")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveProperty("disabled", true);
    await act(async () => {
      fail(new EngineError("invalidInput", "Signature check failed"));
      await Promise.resolve();
    });
    expect((await screen.findByRole("alert")).textContent).toBe("Signature check failed");
  });

  it("shows a failed check before the prompt", async () => {
    vi.spyOn(api, "pluginInstallPrepare").mockRejectedValue(
      new EngineError("invalidInput", "Download didn't match the official list"),
    );
    render(<InstallPrompt plugin={poll} repositoryWarning={false} onClose={vi.fn()} />);
    expect((await screen.findByRole("alert")).textContent).toBe(
      "Download didn't match the official list",
    );
    expect(screen.queryByRole("button", { name: "Install" })).toBeNull();
  });

  it("asks to confirm the first install from a third-party repository", async () => {
    vi.spyOn(api, "pluginInstallPrepare").mockResolvedValue({
      ...offer,
      required: ["net.http"],
      optional: [],
    });
    const plugin = indexed("fetcher", {
      source: { kind: "repo", repositoryId: 3, name: "Fan plugins" },
    });
    render(<InstallPrompt plugin={plugin} repositoryWarning onClose={vi.fn()} />);
    expect(await screen.findByText("Unofficial · Fan plugins")).toBeTruthy();
    expect(screen.getByText("High risk")).toBeTruthy();
    const install = screen.getByRole("button", { name: "Install" });
    expect(install).toHaveProperty("disabled", true);
    fireEvent.click(screen.getByRole("checkbox", { name: "I understand" }));
    expect(install).toHaveProperty("disabled", false);
  });
});
