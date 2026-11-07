import i18next from "i18next";
import { act } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi, type RelayInfo } from "../../api";
import { applyLanguage } from "../../i18n";
import { relayItemId } from "../notifications/centreModel";
import { getCentreState, resetCentre } from "../notifications/centreStore";
import { getToasts, resetNotifications } from "../notifications/notificationStore";
import { bindRelayTrustNotices } from "./relayTrust";

const SERVER = 1;

function info(status: RelayInfo["status"]): RelayInfo {
  return {
    bots: ["Mumbot"],
    host: "relay.example.org",
    status,
    firstSeen: null,
    uses: [],
    broadcastMax: null,
    fingerprint: "AA:BB",
    previousFingerprint: "CC:DD",
  };
}

let api: MockApi;
let unbind: () => void;

beforeEach(async () => {
  await applyLanguage("en");
  resetCentre();
  resetNotifications();
  api = createMockApi();
  setApi(api);
  api.mock.setWindowFocused(true);
  unbind = bindRelayTrustNotices(api, () => ({
    t: i18next.getFixedT("en", "p2p"),
    serverName: () => "Night Owls",
  }));
});

afterEach(() => {
  unbind();
  setApi(undefined);
  vi.restoreAllMocks();
});

const emit = (status: RelayInfo["status"]) => {
  act(() => {
    api.mock.emit("relayChanged", { serverId: SERVER, info: info(status) });
  });
};

describe("relay trust notices (#600)", () => {
  it("adds one centre entry and a toast with the two choices when the relay changed", async () => {
    emit({ kind: "untrusted", keptBlocked: false });
    emit({ kind: "untrusted", keptBlocked: false });
    expect(getCentreState().items.filter((item) => item.kind === "relay")).toHaveLength(1);
    await vi.waitFor(() => {
      expect(getToasts()).toEqual([
        expect.objectContaining({ title: "Relay changed on Night Owls" }),
      ]);
    });
    const toast = getToasts()[0];
    expect(toast?.actions?.map((action) => action.label)).toEqual(["Trust new relay", "Details"]);
  });

  it("never trusts by itself and only the click calls the engine", async () => {
    const trust = vi.spyOn(api, "relayTrustNew");
    emit({ kind: "untrusted", keptBlocked: false });
    await vi.waitFor(() => {
      expect(getToasts()).toHaveLength(1);
    });
    expect(trust).not.toHaveBeenCalled();
    getToasts()[0]?.actions?.[0]?.run();
    expect(trust).toHaveBeenCalledWith(SERVER);
  });

  it("removes the entry once trusted and confirms it", () => {
    emit({ kind: "untrusted", keptBlocked: false });
    emit({ kind: "available" });
    expect(getCentreState().items.some((item) => item.id === relayItemId(SERVER))).toBe(false);
    expect(getToasts().map((toast) => toast.title)).toContain("New relay trusted");
  });

  it("asks nothing for a relay the user kept blocked", () => {
    emit({ kind: "untrusted", keptBlocked: true });
    expect(getCentreState().items).toEqual([]);
  });
});
