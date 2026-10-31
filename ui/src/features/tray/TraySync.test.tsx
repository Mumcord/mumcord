import { act, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MockApi } from "../../api";
import { connectedMock, resetAll } from "../channels/testUtils";
import { resetFollowStore } from "../follow";
import { privateConversation } from "../members";
import { TraySync } from "./TraySync";

let api: MockApi;

afterEach(() => {
  resetFollowStore();
  resetAll();
});

describe("TraySync", () => {
  it("hands unread messages and recent chats to the tray", async () => {
    api = await connectedMock();
    const set = vi.spyOn(api, "trayAttentionSet");
    render(<TraySync onOpenSettings={() => undefined} />);
    await waitFor(() => {
      expect(set).toHaveBeenCalledWith({ unread: 0, mentions: 0, chats: [] });
    });
    const anna = (await api.sessionSnapshot(1))?.users.find((u) => u.name === "Anna");
    if (!anna) throw new Error("seed user missing");
    act(() => {
      api.mock.receiveMessage(privateConversation(anna), "psst", anna.session);
    });
    await waitFor(() => {
      expect(set).toHaveBeenLastCalledWith({
        unread: 1,
        mentions: 0,
        chats: [{ serverId: 1, conversation: privateConversation(anna), name: "Anna", unread: 1 }],
      });
    });
  });

  it("opens the settings when the tray asks", async () => {
    api = await connectedMock();
    const open = vi.fn();
    render(<TraySync onOpenSettings={open} />);
    act(() => {
      api.mock.emit("traySettingsRequested", {});
    });
    expect(open).toHaveBeenCalledOnce();
  });
});
