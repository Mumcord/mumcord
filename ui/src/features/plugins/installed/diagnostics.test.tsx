import { fireEvent, render, screen } from "@testing-library/react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { PluginDiagnostics } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { Diagnostics } from "./Diagnostics";

const api = vi.hoisted(() => ({
  diagnostics: null as PluginDiagnostics | null,
  reenable: vi.fn<(id: string) => Promise<null>>(() => Promise.resolve(null)),
}));
vi.mock("../../../api", async (orig) => ({
  ...(await orig<typeof import("../../../api")>()),
  getApi: () => ({
    pluginDiagnostics: () => Promise.resolve(api.diagnostics),
    settingsGet: () => Promise.resolve({ developerMode: false }),
    pluginReenableWeb: api.reenable,
  }),
}));

function diagnostics(change: Partial<PluginDiagnostics>): PluginDiagnostics {
  return {
    cpuTimeMs: 12,
    memoryBytes: null,
    peakMemoryBytes: null,
    droppedEvents: 0,
    droppedMessages: 0,
    restarts: 0,
    messagesSent: 0,
    messagesRateLimited: 0,
    messagesExpired: 0,
    messagesReceived: 0,
    incomingRateDropped: 0,
    incomingInvalid: 0,
    uiFreezes: 0,
    webDisabled: false,
    loadIssue: null,
    log: [],
    ...change,
  };
}

beforeAll(async () => {
  await applyLanguage("en");
});

beforeEach(() => {
  api.reenable.mockClear();
});

describe("Diagnostics", () => {
  it("counts the freezes while the plugin's web content was open", async () => {
    api.diagnostics = diagnostics({ uiFreezes: 2 });
    render(<Diagnostics id="snacks" />);
    const term = await screen.findByText("Times Mumcord froze while its web content was open");
    expect(term.nextElementSibling).toHaveTextContent("2");
    expect(screen.queryByRole("button", { name: "Turn web content on" })).toBeNull();
  });

  it("turns web content that the watchdog turned off on again", async () => {
    api.diagnostics = diagnostics({ uiFreezes: 1, webDisabled: true });
    const { container } = render(<Diagnostics id="snacks" />);
    const button = await screen.findByRole("button", { name: "Turn web content on" });
    expect(
      screen.getByText(/web content is turned off because Mumcord stopped responding/),
    ).toBeInTheDocument();
    expect(await axeViolations(container)).toEqual([]);
    fireEvent.click(button);
    expect(api.reenable).toHaveBeenCalledWith("snacks");
    await vi.waitFor(() => {
      expect(screen.queryByRole("button", { name: "Turn web content on" })).toBeNull();
    });
  });

  it("lists a plugin refused for its Apps-menu label", async () => {
    api.diagnostics = diagnostics({ loadIssue: { kind: "refused_app_menu_label" } });
    render(<Diagnostics id="badlabel" />);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "This plugin was refused: an Apps-menu label is the same as one of Mumcord's own",
    );
  });

  it("says so when turning web content on fails", async () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    api.reenable.mockImplementationOnce(() => Promise.reject(new Error("io")));
    api.diagnostics = diagnostics({ webDisabled: true });
    render(<Diagnostics id="snacks" />);
    fireEvent.click(await screen.findByRole("button", { name: "Turn web content on" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Web content could not be turned on.",
    );
    expect(screen.getByRole("button", { name: "Turn web content on" })).toBeInTheDocument();
  });
});
