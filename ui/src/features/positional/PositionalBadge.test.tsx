import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../api";
import "../../i18n";
import { PositionalBadge } from "./PositionalBadge";
import { resetPositionalStore } from "./positionalStore";

let api: MockApi;

beforeEach(() => {
  resetPositionalStore();
  api = createMockApi({ seed: "ready" });
  setApi(api);
});

afterEach(() => {
  resetPositionalStore();
  setApi(undefined);
});

describe("PositionalBadge", () => {
  it("is hidden until the person sends positions, then disappears when the flag expires", () => {
    render(<PositionalBadge session={7} />);
    expect(screen.queryByRole("img", { name: "Positional audio active" })).toBeNull();

    act(() => {
      api.mock.emit("positionalSendingChanged", { session: 7, sending: true });
    });
    const badge = screen.getByRole("img", { name: "Positional audio active" });
    expect(badge).toHaveAttribute("title", "Positional audio active");
    expect(badge).toHaveTextContent("3D");

    act(() => {
      api.mock.emit("positionalSendingChanged", { session: 7, sending: false });
    });
    expect(screen.queryByRole("img", { name: "Positional audio active" })).toBeNull();
  });

  it("only shows for the session that sends", () => {
    render(<PositionalBadge session={7} />);
    act(() => {
      api.mock.emit("positionalSendingChanged", { session: 8, sending: true });
    });
    expect(screen.queryByRole("img", { name: "Positional audio active" })).toBeNull();
  });

  it("follows the sender snapshot of a status change", () => {
    render(<PositionalBadge session={7} />);
    act(() => {
      api.mock.emit("positionalStatusChanged", {
        status: { enabled: true, source: "Mumble Link", game: null, senders: [7] },
      });
    });
    expect(screen.getByRole("img", { name: "Positional audio active" })).toBeInTheDocument();
    act(() => {
      api.mock.emit("positionalStatusChanged", {
        status: { enabled: false, source: null, game: null, senders: [] },
      });
    });
    expect(screen.queryByRole("img", { name: "Positional audio active" })).toBeNull();
  });

  it("drops the flag when the person leaves", () => {
    render(<PositionalBadge session={7} />);
    act(() => {
      api.mock.emit("positionalSendingChanged", { session: 7, sending: true });
    });
    act(() => {
      api.mock.emit("userRemoved", { session: 7 });
    });
    expect(screen.queryByRole("img", { name: "Positional audio active" })).toBeNull();
  });
});
