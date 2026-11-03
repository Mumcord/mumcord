import { describe, expect, it } from "vitest";
import { connectedMock, resetAll } from "../channels/testUtils";
import { getServerState } from "../channels/serverStore";
import { canBan } from "./permissions";

describe("the ban list entry", () => {
  it("appears only with the Ban right on the root channel (FR-10.1, FR-10.2)", async () => {
    const api = await connectedMock({ bindStore: true });
    const state = getServerState();
    const root = state.channels.get(0);
    if (!root) throw new Error("the mock has no root channel");
    expect(canBan(1)).toBe(false);

    api.mock.emit("channelUpserted", {
      serverId: 1,
      channel: { ...root, permissions: { ...root.permissions, ban: true } },
    });
    expect(canBan(1)).toBe(true);
    resetAll();
  });
});
