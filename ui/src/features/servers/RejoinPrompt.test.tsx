import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../api";
import "../../i18n";
import { axeViolations } from "../../test/axe";
import { resetServerStore } from "../channels/serverStore";
import { RejoinPrompt } from "./RejoinPrompt";

const SERVER = 1;
const SELF = 1;
const RAID = 2;

let api: MockApi;

function setup(): MockApi {
  resetServerStore();
  api = createMockApi({ seed: "ready" });
  setApi(api);
  render(<RejoinPrompt />);
  return api;
}

async function connect(channelPath: string[] | null = null) {
  await act(async () => {
    await api.connect(SERVER, channelPath, null);
  });
}

const region = () => screen.getByRole("status", { name: "Rejoin last channel" });
const prompt = () => within(region()).queryByText("You were in Raid last time – Rejoin?");
const ownChannel = async () => {
  const snapshot = await api.sessionSnapshot(1);
  return snapshot?.users.find((u) => u.session === SELF)?.channelId;
};

afterEach(() => {
  resetServerStore();
  setApi(undefined);
});

describe("RejoinPrompt", () => {
  it("offers the channel the app last quit in; Rejoin joins it for voice", async () => {
    setup().mock.offerRejoin(SERVER, ["Raid"]);
    await connect();
    await waitFor(() => {
      expect(prompt()).toBeInTheDocument();
    });
    expect(await axeViolations(region())).toEqual([]);
    expect(await ownChannel()).not.toBe(RAID);

    fireEvent.click(within(region()).getByRole("button", { name: "Rejoin" }));
    await waitFor(() => {
      expect(prompt()).not.toBeInTheDocument();
    });
    const snapshot = await api.sessionSnapshot(1);
    const self = snapshot?.users.find((u) => u.session === SELF);
    expect(self?.channelId).toBe(RAID);
    expect(self?.selfDeaf).toBe(false);
  });

  it("Not now hides it and stays read-only; a later connect does not offer again", async () => {
    setup().mock.offerRejoin(SERVER, ["Raid"]);
    await connect();
    await waitFor(() => {
      expect(prompt()).toBeInTheDocument();
    });
    fireEvent.click(within(region()).getByRole("button", { name: "Not now" }));
    expect(prompt()).not.toBeInTheDocument();
    const snapshot = await api.sessionSnapshot(1);
    expect(snapshot?.users.find((u) => u.session === SELF)?.selfDeaf).toBe(true);

    await act(async () => {
      await api.disconnect(1);
    });
    await connect();
    expect(prompt()).not.toBeInTheDocument();
  });

  it("offers nothing without a remembered channel, for a vanished channel or for a link join", async () => {
    setup();
    await connect();
    expect(region()).toBeEmptyDOMElement();

    api.mock.offerRejoin(SERVER, ["Gone"]);
    await connect();
    expect(region()).toBeEmptyDOMElement();

    api.mock.offerRejoin(SERVER, ["Raid"]);
    await connect(["Lobby"]);
    expect(region()).toBeEmptyDOMElement();
  });

  it("disappears when the user joins another channel themselves", async () => {
    setup().mock.offerRejoin(SERVER, ["Raid"]);
    await connect();
    await waitFor(() => {
      expect(prompt()).toBeInTheDocument();
    });
    await act(async () => {
      await api.channelJoin(1, 3);
    });
    expect(prompt()).not.toBeInTheDocument();
  });

  it("shows why a rejoin failed", async () => {
    setup().mock.offerRejoin(SERVER, ["Raid"]);
    await connect();
    await waitFor(() => {
      expect(prompt()).toBeInTheDocument();
    });
    api.mock.failNext("channelJoin", "permissionDenied");
    fireEvent.click(within(region()).getByRole("button", { name: "Rejoin" }));
    expect(
      await within(region()).findByText("You are not allowed to enter this channel."),
    ).toBeInTheDocument();
  });
});
