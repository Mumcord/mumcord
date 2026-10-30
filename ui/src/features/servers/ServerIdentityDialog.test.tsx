import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi, type SavedServer } from "../../api";
import "../../i18n";
import { resetServerStore } from "../channels/serverStore";
import { stubDialog } from "../channels/testUtils";
import { setSettingsOpener } from "../settings/settingsOpener";
import { ServerIdentityDialog } from "./ServerIdentityDialog";
import { ServerMenu } from "./ServerMenu";

let api: MockApi;

function setup(): void {
  stubDialog();
  resetServerStore();
  api = createMockApi({ seed: "fresh" });
  setApi(api);
}

async function serverWithIdentities(): Promise<SavedServer> {
  await api.identityCreate("Zoe", "Default");
  await api.identityCreate("Zoe (Acme)", "Work");
  return api.serverAdd({
    label: "Nordic Gamers",
    address: "nordic.example.org",
    port: 64738,
    username: "Zoe",
    password: { kind: "keep" },
  });
}

function renderDialog(server: SavedServer, onClose = vi.fn()) {
  render(<ServerIdentityDialog server={server} onClose={onClose} />);
  return onClose;
}

const combobox = () => screen.getByRole("combobox", { name: "Identity for this server" });
const option = (name: string) => screen.getByRole("option", { name });

function choose(name: string): void {
  fireEvent.click(combobox());
  fireEvent.click(option(name));
}

beforeEach(setup);
afterEach(() => {
  setSettingsOpener(null);
  vi.restoreAllMocks();
});

describe("ServerIdentityDialog", () => {
  it("offers the default and every identity", async () => {
    const server = await serverWithIdentities();
    renderDialog(server);
    fireEvent.click(await screen.findByRole("combobox", { name: "Identity for this server" }));
    expect(screen.getByRole("option", { name: "Use default (Default)" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Default (Zoe)" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Work (Zoe (Acme))" })).toBeInTheDocument();
  });

  it("saves the choice without a question while the server is not connected", async () => {
    const server = await serverWithIdentities();
    const setIdentity = vi.spyOn(api, "serverSetIdentity");
    const onClose = renderDialog(server);
    await screen.findByRole("combobox", { name: "Identity for this server" });
    choose("Work (Zoe (Acme))");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(setIdentity).toHaveBeenCalledWith(server.id, 2);
    });
    expect(onClose).toHaveBeenCalled();
    expect((await api.serversList())[0]?.identityId).toBe(2);
  });

  it("asks before reconnecting a connected server and can wait for later", async () => {
    const server = await serverWithIdentities();
    await api.connect(server.id, null, null);
    const disconnect = vi.spyOn(api, "disconnect");
    renderDialog(server);
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Save and reconnect" })).toBeInTheDocument();
    });
    choose("Work (Zoe (Acme))");
    fireEvent.click(screen.getByRole("button", { name: "Save and reconnect" }));
    const question = await screen.findByRole("dialog", { name: "Reconnect Nordic Gamers?" });
    expect(question).toHaveTextContent("Work");

    fireEvent.click(within(question).getByRole("button", { name: "Later" }));
    await waitFor(async () => {
      expect((await api.serversList())[0]?.identityId).toBe(2);
    });
    expect(disconnect).not.toHaveBeenCalled();
  });

  it("reconnects only the changed server", async () => {
    const server = await serverWithIdentities();
    const other = await api.serverAdd({
      label: "Other",
      address: "other.example.org",
      port: 64738,
      username: "Zoe",
      password: { kind: "keep" },
    });
    await api.connect(server.id, null, null);
    await api.connect(other.id, null, null);
    const connect = vi.spyOn(api, "connect");
    const disconnect = vi.spyOn(api, "disconnect");
    renderDialog(server);
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Save and reconnect" })).toBeInTheDocument();
    });
    choose("Work (Zoe (Acme))");
    fireEvent.click(screen.getByRole("button", { name: "Save and reconnect" }));
    fireEvent.click(await screen.findByRole("button", { name: "Reconnect" }));
    await waitFor(() => {
      expect(disconnect).toHaveBeenCalledWith(server.id);
    });
    expect(disconnect).not.toHaveBeenCalledWith(other.id);
    await waitFor(() => {
      expect(connect).toHaveBeenCalledWith(server.id, null, null);
    });
    expect(connect).not.toHaveBeenCalledWith(other.id, null, null);
  });

  it("leads to the identity manager in the settings", async () => {
    const server = await serverWithIdentities();
    const open = vi.fn();
    setSettingsOpener(open);
    renderDialog(server);
    fireEvent.click(await screen.findByRole("button", { name: "Manage identities…" }));
    expect(open).toHaveBeenCalledWith("identity");
  });
});

describe("server menu", () => {
  it("offers the identity dialog entry", async () => {
    const server = await serverWithIdentities();
    const onSelect = vi.fn();
    render(
      <ServerMenu
        label="Options"
        items={[
          { id: "identity", label: "Identity…", onSelect },
          { id: "edit", label: "Edit server", onSelect: vi.fn() },
        ]}
        position={{ x: 0, y: 0 }}
        onClose={vi.fn()}
      />,
    );
    expect(server.identityId).toBeNull();
    fireEvent.click(screen.getByRole("menuitem", { name: "Identity…" }));
    expect(onSelect).toHaveBeenCalled();
  });
});
