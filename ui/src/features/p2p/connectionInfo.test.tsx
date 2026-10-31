import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { createMockApi, setApi } from "../../api";
import "../../i18n";
import { ConnectionQuality } from "../connection/ConnectionQuality";

afterEach(() => {
  setApi(undefined);
});

describe("connection info of a P2P connection (FR-4.19)", () => {
  it("shows the relay path in the person's connection details and follows changes", async () => {
    const api = createMockApi({ seed: "demo" });
    setApi(api);
    render(<ConnectionQuality session={2} serverId={1} />);
    fireEvent.click(await screen.findByRole("button", { name: /^Connection: / }));
    const details = await screen.findByLabelText("Connection");
    expect(await within(details).findByText("Peer-to-peer")).toBeInTheDocument();
    expect(details).toHaveTextContent("Via relay (TCP 443) · IP hidden");
    act(() => {
      api.mock.emit("p2pLinkChanged", {
        serverId: 1,
        session: 2,
        link: { path: "relayUdp", ipHidden: false },
      });
    });
    expect(details).toHaveTextContent("Via relay (UDP) · IP visible");
  });
});
