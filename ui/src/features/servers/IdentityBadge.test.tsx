import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../api";
import "../../i18n";
import { resetServerStore, showServer } from "../channels/serverStore";
import { IdentityBadge } from "./IdentityBadge";

let api: MockApi;

beforeEach(() => {
  resetServerStore();
  api = createMockApi({ seed: "fresh" });
  setApi(api);
});

afterEach(() => {
  resetServerStore();
  setApi(undefined);
});

async function twoServers(): Promise<void> {
  await api.identityCreate("Zoe", "Default");
  await api.identityCreate("Zoe (Acme)", "Work");
  const first = await api.serverAdd({
    label: "Nordic Gamers",
    address: "nordic.example.org",
    port: 64738,
    username: "Zoe",
    password: { kind: "keep" },
  });
  const second = await api.serverAdd({
    label: "Stream Lounge",
    address: "stream.example.org",
    port: 64738,
    username: "Zoe",
    password: { kind: "keep" },
  });
  await api.serverSetIdentity(second.id, 2);
  showServer(first.id);
}

describe("IdentityBadge", () => {
  it("stays hidden while the shown server uses the default identity", async () => {
    await twoServers();
    const { container } = render(<IdentityBadge className="badge" />);
    await waitFor(() => {
      expect(container).toBeEmptyDOMElement();
    });
  });

  it("names the identity of a server that uses its own", async () => {
    await twoServers();
    const [first, second] = await api.serversList();
    showServer((second ?? first ?? { id: 0 }).id);
    render(<IdentityBadge className="badge" />);
    const badge = await screen.findByText("Identity: Work");
    expect(badge).toHaveAccessibleName("Connected with the identity Work");
  });
});
