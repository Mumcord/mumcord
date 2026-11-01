import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { PluginsSection } from "../PluginsSection";
import { resetPluginStore } from "../pluginStore";
import { resetPluginRequestStore, routePluginRequest } from "../pluginRequestStore";
import { RepositoriesTab } from "./RepositoriesTab";

let api: MockApi;

beforeEach(async () => {
  await applyLanguage("en");
  resetPluginStore();
  resetPluginRequestStore();
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
});

async function addExample(url = "https://example.org/index.json", key = "") {
  fireEvent.click(await screen.findByRole("button", { name: "Add repository" }));
  const dialog = await screen.findByRole("dialog");
  fireEvent.change(within(dialog).getByLabelText("Repository address"), { target: { value: url } });
  if (key !== "") {
    fireEvent.change(within(dialog).getByLabelText("Expected key (optional)"), {
      target: { value: key },
    });
  }
  fireEvent.click(within(dialog).getByRole("button", { name: "Check repository" }));
  return dialog;
}

describe("RepositoriesTab", () => {
  it("shows name, fingerprint and warning before adding, then lists the repository", async () => {
    const add = vi.spyOn(api, "pluginRepoAdd");
    render(<RepositoriesTab />);
    const dialog = await addExample();
    expect(await within(dialog).findByText("Example repository")).toBeInTheDocument();
    expect(within(dialog).getByText("1CB49831E26E61AD")).toBeInTheDocument();
    expect(within(dialog).getByText("https://example.org")).toBeInTheDocument();
    expect(within(dialog).getByText(/not reviewed by Mumcord/)).toBeInTheDocument();
    expect(add).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("button", { name: "Add repository" }));
    const list = await screen.findByRole("list", { name: "Added repositories" });
    expect(within(list).getByText("Example repository")).toBeInTheDocument();
    expect(add).toHaveBeenCalledWith("https://example.org/index.json", null);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("shows why a repository is refused", async () => {
    render(<RepositoriesTab />);
    await addExample("http://example.org/index.json");
    expect(await screen.findByRole("alert")).toHaveTextContent("must start with https://");
  });

  it("refuses a wrong Expected key", async () => {
    render(<RepositoriesTab />);
    await addExample("https://example.org/index.json", "WRONG");
    expect(await screen.findByRole("alert")).toHaveTextContent("doesn't match");
  });

  it("stops at ten repositories", async () => {
    for (let i = 0; i < 10; i++)
      await api.pluginRepoAdd(`https://example.org/${String(i)}.json`, null);
    render(<RepositoriesTab />);
    expect(await screen.findByText("The limit of 10 repositories is reached.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add repository" })).toBeDisabled();
  });

  it("switches a repository off and removes it after confirming", async () => {
    await api.pluginRepoAdd("https://example.org/index.json", null);
    const setEnabled = vi.spyOn(api, "pluginRepoSetEnabled");
    render(<RepositoriesTab />);
    fireEvent.click(await screen.findByRole("switch", { name: "Use Example repository" }));
    await waitFor(() => {
      expect(setEnabled).toHaveBeenCalledWith(1, false);
    });
    fireEvent.click(screen.getByRole("button", { name: "Remove Example repository" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Remove" }));
    await waitFor(() => {
      expect(screen.queryByRole("list", { name: "Added repositories" })?.children.length ?? 0).toBe(
        0,
      );
    });
  });

  it("asks about a changed key and pins the new one on request", async () => {
    await api.pluginRepoAdd("https://example.org/index.json", null);
    vi.spyOn(api, "pluginReposList").mockResolvedValueOnce([
      {
        id: 1,
        url: "https://example.org/index.json",
        name: "Example repository",
        fingerprint: "1CB49831E26E61AD",
        enabled: false,
        keyChanged: true,
        lastChecked: null,
        installedPlugins: 0,
      },
    ]);
    const accept = vi.spyOn(api, "pluginRepoAcceptKey");
    render(<RepositoriesTab />);
    expect(await screen.findByText("Key changed, paused")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Review key change" }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText("0F0F0F0F0F0F0F0F")).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Trust new key" }));
    await waitFor(() => {
      expect(accept).toHaveBeenCalledWith(1);
    });
  });

  it("opens the Add dialog with the announced key when a server suggests a repository", async () => {
    render(<PluginsSection developerMode={false} />);
    act(() => {
      routePluginRequest({
        kind: "addRepository",
        url: "https://example.org/r.json",
        key: "MOCKKEY",
      });
    });
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByLabelText("Repository address")).toHaveValue(
      "https://example.org/r.json",
    );
    expect(within(dialog).getByLabelText("Expected key (optional)")).toHaveValue("MOCKKEY");
    expect(screen.getByRole("tab", { name: "Repositories" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("saves and resets the mirror under Advanced", async () => {
    const onChange = vi.fn();
    render(<RepositoriesTab mirror={{ url: null, onChange }} />);
    fireEvent.click(await screen.findByText("Advanced"));
    fireEvent.change(screen.getByLabelText("Mirror for the official plugin list (optional)"), {
      target: { value: " https://mirror.example.org/ " },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(onChange).toHaveBeenCalledWith("https://mirror.example.org/");
    fireEvent.click(screen.getByRole("button", { name: "Use the default" }));
    expect(onChange).toHaveBeenLastCalledWith(null);
  });

  it("has no accessibility violations", async () => {
    await api.pluginRepoAdd("https://example.org/index.json", null);
    const { container } = render(<RepositoriesTab mirror={{ url: null, onChange: vi.fn() }} />);
    await screen.findByText("Example repository");
    expect(await axeViolations(container)).toEqual([]);
  });
});
