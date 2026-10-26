import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../api";
import "../../i18n";
import { axeViolations } from "../../test/axe";
import { resetServerStore } from "../channels/serverStore";
import { resetFollowStore } from "../follow/followStore";
import { stubDialog } from "../channels/testUtils";
import { resetMentions } from "./mentions";
import { ConnectionStatus } from "./ConnectionStatus";
import { ServersRoot } from "./ServersRoot";

let api: MockApi;

async function setup(saved: string | null, withSecond = false): Promise<number> {
  resetServerStore();
  api = createMockApi();
  setApi(api);
  const server = await api.serverAdd({
    label: "Raid Night",
    address: "raid.example.org",
    port: 64738,
    username: "You",
    password: saved === null ? { kind: "keep" } : { kind: "set", value: saved },
  });
  api.mock.requirePassword(server.id, "right");
  if (withSecond) {
    const second = await api.serverAdd({
      label: "Movie Night",
      address: "movies.example.org",
      port: 64738,
      username: "You",
      password: { kind: "set", value: "old" },
    });
    api.mock.requirePassword(second.id, "other");
  }
  render(
    <>
      <nav aria-label="Servers">
        <ServersRoot />
      </nav>
      <main>
        <ConnectionStatus />
      </main>
    </>,
  );
  await waitFor(() => {
    expect(screen.queryByRole("button", { name: "Add a server" })).toBeInTheDocument();
  });
  return server.id;
}

const connectTo = (name: string) => {
  fireEvent.click(screen.getByRole("button", { name }));
};
const dialog = () => screen.findByRole("dialog", { name: "Password for Raid Night" });
const field = () => screen.getByLabelText("Password");
const submit = (password: string) => {
  fireEvent.change(field(), { target: { value: password } });
  fireEvent.click(screen.getByRole("button", { name: "Connect" }));
};

beforeEach(() => {
  stubDialog();
});

afterEach(() => {
  resetServerStore();
  resetFollowStore();
  resetMentions();
  setApi(undefined);
});

describe("PasswordDialog", () => {
  it("opens focused and masked, with Save password pre-checked for a saved password", async () => {
    await setup("old");
    connectTo("Raid Night");
    const view = await dialog();
    expect(field()).toHaveFocus();
    expect(field()).toHaveAttribute("type", "password");
    expect(screen.getByRole("checkbox", { name: "Save password" })).toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: "Show" }));
    expect(field()).toHaveAttribute("type", "text");
    expect(await axeViolations(view)).toEqual([]);
  });

  it("starts unchecked without a saved password", async () => {
    await setup(null);
    connectTo("Raid Night");
    await dialog();
    expect(screen.getByRole("checkbox", { name: "Save password" })).not.toBeChecked();
  });

  it("shows an inline error for a second wrong password, then connects and saves", async () => {
    await setup("old");
    connectTo("Raid Night");
    await dialog();
    submit("nope");
    expect(await screen.findByRole("alert")).toHaveTextContent("That password is wrong too");
    expect(await dialog()).toBeInTheDocument();
    submit("right");
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
  });

  it("keeps the saved password when Save password is off", async () => {
    await setup("old");
    const update = vi.spyOn(api, "serverUpdate");
    connectTo("Raid Night");
    await dialog();
    fireEvent.click(screen.getByRole("checkbox", { name: "Save password" }));
    submit("right");
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
    expect(update).not.toHaveBeenCalled();
  });

  it("saves the typed password only after the connect succeeded", async () => {
    await setup("old");
    const update = vi.spyOn(api, "serverUpdate");
    connectTo("Raid Night");
    await dialog();
    submit("nope");
    await screen.findByRole("alert");
    expect(update).not.toHaveBeenCalled();
    submit("right");
    await waitFor(() => {
      expect(update).toHaveBeenCalledTimes(1);
    });
  });

  it("leaves the server disconnected with the normal error on Cancel and Esc", async () => {
    await setup("old");
    connectTo("Raid Night");
    const first = await dialog();
    fireEvent.click(within(first).getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
    expect(screen.getByRole("status", { name: "Connection status" })).toHaveTextContent(
      "The server password is wrong.",
    );
    connectTo("Raid Night");
    const second = await dialog();
    fireEvent(second, new Event("cancel", { cancelable: true }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
  });
});

describe("PasswordDialog with several servers and reconnects", () => {
  it("gives each server refused at once its own dialog, one after the other", async () => {
    await setup("old", true);
    connectTo("Raid Night");
    fireEvent.click(screen.getByRole("button", { name: /Movie Night/ }));
    await dialog();
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    submit("right");
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Password for Raid Night" })).toBeNull();
    });
    // Raid Night's own password did not satisfy Movie Night, which now has its turn.
    const movie = await screen.findByRole("dialog", { name: "Password for Movie Night" });
    expect(movie).toBeInTheDocument();
    fireEvent.change(field(), { target: { value: "other" } });
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
  });

  it("never opens for a refusal while reconnecting", async () => {
    const id = await setup("right");
    connectTo("Raid Night");
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /Raid Night/ })).toHaveAttribute(
        "aria-current",
        "true",
      );
    });
    act(() => {
      api.mock.loseConnection();
      api.mock.emit("connectionStateChanged", {
        serverId: id,
        state: { state: "disconnected", error: { code: "wrongPassword", message: "" } },
      });
    });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
