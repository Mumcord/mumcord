import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../api";
import "../../i18n";
import { axeViolations } from "../../test/axe";
import { resetServerStore } from "../channels/serverStore";
import { ServersRoot } from "../servers/ServersRoot";
import { toggleServerBrowser, useServerBrowserOpen } from "./browserStore";
import { ServerBrowser } from "./ServerBrowser";

let api: MockApi;

function BrowserWhenOpen() {
  return useServerBrowserOpen() ? <ServerBrowser /> : null;
}

function setup() {
  resetServerStore();
  api = createMockApi({ seed: "ready" });
  setApi(api);
  const list = vi.spyOn(api, "publicServersList");
  const ping = vi.spyOn(api, "publicServerPing");
  const view = render(
    <>
      <nav aria-label="Servers">
        <ServersRoot />
      </nav>
      <BrowserWhenOpen />
    </>,
  );
  return { list, ping, view };
}

const rows = () => screen.getAllByRole("row").slice(1);
const rowOf = (name: string) => screen.getByRole("rowheader", { name: new RegExp(name) });
const cells = (name: string) => within(screen.getByRole("row", { name: new RegExp(`^${name}`) }));

afterEach(() => {
  act(() => {
    if (screen.queryByRole("region", { name: "Public servers" })) toggleServerBrowser();
  });
  resetServerStore();
  setApi(undefined);
});

describe("ServerBrowser", () => {
  it("loads the list only when the browser opens", async () => {
    const { list } = setup();
    const browse = await screen.findByRole("button", { name: "Browse public servers" });
    expect(list).not.toHaveBeenCalled();
    fireEvent.click(browse);
    expect(browse).toHaveAttribute("aria-pressed", "true");
    await waitFor(() => {
      expect(rows().length).toBeGreaterThan(5);
    });
    expect(list).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("region", { name: "Public servers" })).toHaveTextContent(
      /\d+ of \d+ shown/,
    );
  });

  it("pings the rows on screen and shows the answers", async () => {
    const { ping } = setup();
    act(toggleServerBrowser);
    await waitFor(() => {
      expect(cells("Nordic Gamers").getByText("38 ms")).toBeVisible();
    });
    expect(cells("Nordic Gamers").getByText("24 / 60")).toBeVisible();
    expect(ping.mock.calls.every(([host]) => host.endsWith(".example.org"))).toBe(true);
  });

  it("filters by search text and hides full servers", async () => {
    setup();
    act(toggleServerBrowser);
    await waitFor(() => {
      expect(rowOf("Night Raiders")).toBeVisible();
    });
    fireEvent.change(screen.getByRole("searchbox", { name: /Search/ }), {
      target: { value: "harbor" },
    });
    expect(rows()).toHaveLength(1);
    fireEvent.change(screen.getByRole("searchbox", { name: /Search/ }), {
      target: { value: "" },
    });
    await waitFor(() => {
      expect(cells("Night Raiders").getByText("Full")).toBeVisible();
    });
    fireEvent.click(screen.getByRole("button", { name: "Hide full" }));
    expect(screen.queryByRole("rowheader", { name: /Night Raiders/ })).toBeNull();
  });

  it("adds a server to the favourites in one click", async () => {
    setup();
    act(toggleServerBrowser);
    const add = await screen.findByRole("button", {
      name: "Add Quiet Library to your servers",
    });
    fireEvent.click(add);
    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Quiet Library is in your servers" }),
      ).toBeDisabled();
    });
    const saved = await api.serversList();
    expect(
      saved.some((s) => s.address === "library.example.org" && s.label === "Quiet Library"),
    ).toBe(true);
  });

  it("connects in one click and closes the browser", async () => {
    setup();
    const connect = vi.spyOn(api, "connect");
    act(toggleServerBrowser);
    fireEvent.click(await screen.findByRole("button", { name: "Connect to Retro LAN" }));
    await waitFor(() => {
      expect(connect).toHaveBeenCalled();
    });
    expect(screen.queryByRole("region", { name: "Public servers" })).toBeNull();
    const saved = await api.serversList();
    expect(saved.find((s) => s.id === connect.mock.calls[0]?.[0])?.address).toBe(
      "retrolan.example.org",
    );
  });

  it("offers a retry when the list cannot be loaded", async () => {
    const { list } = setup();
    list.mockRejectedValueOnce({ code: "serverUnreachable", message: "offline" });
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    act(toggleServerBrowser);
    fireEvent.click(await screen.findByRole("button", { name: "Try again" }));
    await waitFor(() => {
      expect(rows().length).toBeGreaterThan(5);
    });
    expect(list).toHaveBeenCalledTimes(2);
  });

  it("has no accessibility violations", async () => {
    const { view } = setup();
    act(toggleServerBrowser);
    await waitFor(() => {
      expect(rows().length).toBeGreaterThan(5);
    });
    expect(await axeViolations(view.container)).toEqual([]);
  });
});
