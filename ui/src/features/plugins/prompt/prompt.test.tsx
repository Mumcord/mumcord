import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createMockApi,
  EngineError,
  setApi,
  type MockApi,
  type PluginInstallGate,
  type PluginInstallOffer,
  type PluginInstallScore,
  type PluginScored,
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
  origins: [],
  check: {
    kind: "ready",
    score: { score: { kind: "official" }, listing: null, higherThanListing: false },
    gate: { confirm: null, blockedBySafety: false, reasons: [] },
  },
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
    render(<InstallPrompt plugin={repo} onClose={vi.fn()} />);
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
    render(<InstallPrompt plugin={poll} onClose={vi.fn()} />);
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
        <InstallPrompt plugin={poll} onClose={onClose} />
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
    expect(install).toHaveBeenCalledWith("poll", "1.1.0", ["ui.panel"], "everyone", true);
    expect(screen.getByText("Poll installed")).toBeTruthy();
  });

  it("hides the visibility choice without peers.messages", async () => {
    vi.spyOn(api, "pluginInstallPrepare").mockResolvedValue({
      ...offer,
      required: ["ui.chat-card"],
      optional: [],
    });
    render(<InstallPrompt plugin={poll} onClose={vi.fn()} />);
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
    render(<InstallPrompt plugin={poll} onClose={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "Install" }));
    act(() => {
      api.mock.emit("pluginsInstallProgress", { id: "poll", progress: { stage: "verifying" } });
    });
    expect(
      (await screen.findByRole("progressbar", { name: "Installing Poll" })).getAttribute(
        "aria-valuetext",
      ),
    ).toBe("100% · Checking signature…");
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
    render(<InstallPrompt plugin={poll} onClose={vi.fn()} />);
    expect((await screen.findByRole("alert")).textContent).toBe(
      "Download didn't match the official list",
    );
    expect(screen.queryByRole("button", { name: "Install" })).toBeNull();
  });

  it("shows the download progress while preparing and retries after a failure", async () => {
    const prepare = vi
      .spyOn(api, "pluginInstallPrepare")
      .mockRejectedValueOnce(new EngineError("invalidInput", "The plugin list is unreachable"))
      .mockReturnValueOnce(new Promise(() => {}));
    render(<InstallPrompt plugin={poll} onClose={vi.fn()} />);
    expect((await screen.findByRole("alert")).textContent).toBe("The plugin list is unreachable");
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(prepare).toHaveBeenCalledTimes(2);
    act(() => {
      api.mock.emit("pluginsInstallProgress", {
        id: "poll",
        progress: { stage: "downloading", received: 11_500_000, total: 23_000_000 },
      });
    });
    const bar = await screen.findByRole("progressbar", { name: "Installing Poll" });
    expect(bar.getAttribute("aria-valuenow")).toBe("50");
    expect(screen.getByText("Downloading 11.5 MB of 23 MB")).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});

const fanRepo = { kind: "repo", repositoryId: 3, name: "Meeple Works" } as const;

function scored(value: number, band: "low" | "medium" | "high", newKey = false): PluginScored {
  return {
    kind: "scored",
    score: {
      value,
      band,
      e: value,
      f: null,
      h: 0,
      version: 1,
      factors: [
        { kind: "networkFloor", effect: { kind: "points", value: 20 }, detailKey: "network" },
        { kind: "webSurface", effect: { kind: "points", value: 10 }, detailKey: "web" },
        {
          kind: "keyAge",
          effect: { kind: "percent", value: newKey ? 10 : 0 },
          detailKey: newKey ? "key_age.new" : "key_age.known",
        },
      ],
    },
  };
}

function gated(
  score: PluginScored,
  gate: Partial<PluginInstallGate>,
  extra: Partial<PluginInstallScore> = {},
): PluginInstallOffer {
  return {
    ...offer,
    id: "brawl",
    version: "2.1.0",
    required: ["ui.webview", "net.origins", "ui.stage"],
    origins: [
      { origin: "https://api.boardbrawl.example", purpose: "accounts and match history" },
      { origin: "wss://play.boardbrawl.example", purpose: "live match connection" },
    ],
    optional: [{ id: "peers.relay", suggested: false }],
    check: {
      kind: "ready",
      score: { score, listing: null, higherThanListing: false, ...extra },
      gate: { confirm: null, blockedBySafety: false, reasons: [], ...gate },
    },
  };
}

const brawl = indexed("brawl", {
  name: "Board Brawl",
  source: fanRepo,
  origins: ["https://api.boardbrawl.example", "wss://play.boardbrawl.example"],
});

describe("InstallPrompt gate", () => {
  it("shows the websites, the own code and one I-understand box, and sends the tick", async () => {
    vi.spyOn(api, "pluginInstallPrepare").mockResolvedValue(
      gated(scored(41, "medium"), {
        confirm: "understand",
        reasons: ["ownCodeAndWebsites", "repositoryWarning"],
      }),
    );
    const install = vi.spyOn(api, "pluginInstall").mockResolvedValue(null);
    render(<InstallPrompt plugin={brawl} onClose={vi.fn()} />);
    const dialog = await screen.findByRole("dialog", { name: "Install Board Brawl?" });
    expect(screen.getByText("https://api.boardbrawl.example")).toBeTruthy();
    expect(
      screen.getByText(
        "These websites can see your IP address and anything Board Brawl sends them.",
      ),
    ).toBeTruthy();
    expect(
      screen.getByText("Runs code made by Meeple Works. Mumcord has not reviewed it."),
    ).toBeTruthy();
    expect(
      screen.getByText(
        "It can't see Mumcord, your files, microphone, camera or clipboard. Shown only in: media area.",
      ),
    ).toBeTruthy();
    expect(
      screen.getByRole("group", { name: "This plugin brings its own code and talks to websites." }),
    ).toBeTruthy();
    expect(screen.getByText(/Meeple Works is a repository you added/)).toBeTruthy();
    expect(screen.getByRole("meter", { name: "Security score" })).toBeTruthy();
    expect(await axeViolations(dialog)).toEqual([]);

    const button = screen.getByRole("button", { name: "Install" });
    expect(button).toHaveProperty("disabled", true);
    fireEvent.click(screen.getByRole("checkbox", { name: "I understand" }));
    expect(button).toHaveProperty("disabled", false);
    fireEvent.click(button);
    await waitFor(() => {
      expect(install).toHaveBeenCalledWith("brawl", "2.1.0", [], "friends", true);
    });
  });

  it("asks a High plugin to be installed anyway, explained from its real facts", async () => {
    vi.spyOn(api, "pluginInstallPrepare").mockResolvedValue(
      gated(scored(72, "high", true), {
        confirm: "highRisk",
        reasons: ["highRisk", "ownCodeAndWebsites"],
      }),
    );
    render(<InstallPrompt plugin={brawl} onClose={vi.fn()} />);
    const dialog = await screen.findByRole("dialog", { name: "Install Board Brawl?" });
    expect(screen.getByRole("group", { name: "This is a high-risk plugin" })).toBeTruthy();
    expect(
      screen.getByText(
        /^It can open its own web pages and talk to 2 websites you can't check\. Mumcord cannot see .* It was published by someone who is new to this computer\. Only install it if you know and trust its author\./,
      ),
    ).toBeTruthy();
    expect(await axeViolations(dialog)).toEqual([]);
    const button = screen.getByRole("button", { name: "Install anyway" });
    expect(button).toHaveProperty("disabled", true);
    fireEvent.click(
      screen.getByRole("checkbox", {
        name: "I understand the risk and I trust the author of Board Brawl",
      }),
    );
    expect(button).toHaveProperty("disabled", false);
  });

  it("opens the full breakdown from the score card", async () => {
    vi.spyOn(api, "pluginInstallPrepare").mockResolvedValue(gated(scored(41, "medium"), {}));
    render(<InstallPrompt plugin={brawl} onClose={vi.fn()} />);
    const more = await screen.findByRole("button", { name: "Show the full breakdown" });
    expect(more.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(more);
    expect(more.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getAllByRole("table").length).toBeGreaterThan(0);
    expect(await axeViolations(screen.getByRole("dialog"))).toEqual([]);
  });

  it("says when the package scores higher than its listing", async () => {
    vi.spyOn(api, "pluginInstallPrepare").mockResolvedValue(
      gated(scored(41, "medium"), {}, { listing: scored(20, "low"), higherThanListing: true }),
    );
    render(<InstallPrompt plugin={brawl} onClose={vi.fn()} />);
    expect(await screen.findByText("This plugin scores higher than its listing said")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Install" })).toHaveProperty("disabled", false);
  });

  it("keeps Install disabled without a box when the safety setting blocks it", async () => {
    vi.spyOn(api, "pluginInstallPrepare").mockResolvedValue(
      gated(scored(72, "high"), {
        confirm: "highRisk",
        blockedBySafety: true,
        reasons: ["highRisk"],
      }),
    );
    render(<InstallPrompt plugin={brawl} onClose={vi.fn()} />);
    const dialog = await screen.findByRole("dialog", { name: "Install Board Brawl?" });
    expect(screen.getByText("Blocked by your safety setting")).toBeTruthy();
    expect(screen.queryByRole("checkbox", { name: /I understand/ })).toBeNull();
    expect(screen.getByRole("button", { name: "Install" })).toHaveProperty("disabled", true);
    expect(await axeViolations(dialog)).toEqual([]);
  });

  it("refuses a hard stop with its reason, no score and no Install button", async () => {
    vi.spyOn(api, "pluginInstallPrepare").mockResolvedValue({
      ...offer,
      required: [],
      optional: [],
      check: {
        kind: "refused",
        stop: "unsafeOrigin",
        reason: "Cannot be installed: it wants to use a website that is not encrypted.",
      },
    });
    render(<InstallPrompt plugin={brawl} onClose={vi.fn()} />);
    const dialog = await screen.findByRole("dialog", { name: "Install Board Brawl?" });
    expect(
      await screen.findByText(
        "Cannot be installed: it wants to use a website that is not encrypted.",
      ),
    ).toBeTruthy();
    expect(screen.queryByRole("meter")).toBeNull();
    expect(screen.queryByRole("button", { name: "Install" })).toBeNull();
    expect(await axeViolations(dialog)).toEqual([]);
  });

  it("does not download a plugin that needs a newer Mumcord", async () => {
    const prepare = vi.spyOn(api, "pluginInstallPrepare");
    render(<InstallPrompt plugin={{ ...brawl, needsMumcord: "0.8.0" }} onClose={vi.fn()} />);
    const dialog = screen.getByRole("dialog", { name: "Install Board Brawl?" });
    expect(screen.getByText("Needs Mumcord 0.8.0 or newer")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Install" })).toHaveProperty("disabled", true);
    expect(prepare).not.toHaveBeenCalled();
    expect(await axeViolations(dialog)).toEqual([]);
  });

  it("shows no score for an official plugin", async () => {
    vi.spyOn(api, "pluginInstallPrepare").mockResolvedValue(offer);
    render(<InstallPrompt plugin={poll} onClose={vi.fn()} />);
    await screen.findAllByText("Required");
    expect(screen.queryByRole("meter")).toBeNull();
    expect(screen.queryByRole("checkbox", { name: /I understand/ })).toBeNull();
  });
});
