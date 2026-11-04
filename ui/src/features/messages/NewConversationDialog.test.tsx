import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { getServerState } from "../channels/serverStore";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { NewConversationDialog } from "./NewConversationDialog";

beforeEach(stubDialog);
afterEach(resetAll);

describe("NewConversationDialog", () => {
  it("offers voice bots and people, never a bot without voice (issue #458)", async () => {
    const api = await connectedMock({ bindStore: true });
    const anna = getServerState().users.get(2);
    if (!anna) throw new Error("Anna is missing");
    act(() => {
      api.mock.upsertUser({
        ...anna,
        session: 20,
        name: "Relay",
        bot: { voice: false, modules: ["peer"] },
      });
      api.mock.upsertUser({
        ...anna,
        session: 21,
        name: "DJ Byte",
        bot: { voice: true, modules: ["music"] },
      });
    });
    render(<NewConversationDialog onClose={() => undefined} />);
    expect(screen.getByText("Anna")).toBeInTheDocument();
    expect(screen.getByText("DJ Byte")).toBeInTheDocument();
    expect(screen.queryByText("Relay")).not.toBeInTheDocument();
  });
});
