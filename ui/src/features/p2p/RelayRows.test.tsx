import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type RelayInfo } from "../../api";
import "../../i18n";
import { axeViolations } from "../../test/axe";
import { RelayRows, RelayRowsView } from "./RelayRows";

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  );
}

const RELAY: RelayInfo = {
  bots: ["RelayBot"],
  host: "relay.example.org",
  status: { kind: "available" },
  firstSeen: "2026-09-01T18:00:00Z",
};

function view(info: RelayInfo, onAction = vi.fn()) {
  return render(
    <dl>
      <RelayRowsView info={info} failed={false} onAction={onAction} Row={Row} />
    </dl>,
  );
}

function valueOf(label: string): string | null | undefined {
  return screen.getByText(label).closest("div")?.querySelector("dd")?.textContent;
}

afterEach(() => {
  setApi(undefined);
});

describe("RelayRowsView", () => {
  it("says No twice without an announcement (AC-4.1)", () => {
    view({ bots: [], host: null, status: { kind: "none" }, firstSeen: null });
    expect(valueOf("Mumbot")).toBe("No");
    expect(valueOf("Mumpeer relay")).toBe("No");
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("names the bot and the relay host and offers to forget the pin (AC-4.2)", async () => {
    const onAction = vi.fn();
    const { container } = view(RELAY, onAction);
    expect(valueOf("Mumbot")).toBe("Yes (RelayBot)");
    expect(valueOf("Mumpeer relay")).toBe("Available · relay.example.org" + "Forget relay");
    fireEvent.click(screen.getByRole("button", { name: "Forget relay" }));
    expect(onAction).toHaveBeenCalledWith("forget");
    expect(await axeViolations(container)).toEqual([]);
  });

  it("explains an unavailable relay (AC-4.11)", () => {
    view({ ...RELAY, status: { kind: "unavailable", reason: { kind: "botLeft" } } });
    expect(screen.getByText("Unavailable: The relay bot left the server.")).toBeInTheDocument();
  });

  it("asks about a changed relay (AC-4.13)", () => {
    const onAction = vi.fn();
    view({ ...RELAY, status: { kind: "untrusted", keptBlocked: false } }, onAction);
    expect(screen.getByText("Changed, not trusted")).toBeInTheDocument();
    expect(screen.getByText("The relay of this server changed.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Forget relay" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Keep blocked" }));
    expect(onAction).toHaveBeenCalledWith("keepBlocked");
    fireEvent.click(screen.getByRole("button", { name: "Trust new relay" }));
    expect(onAction).toHaveBeenCalledWith("trustNew");
  });

  it("only offers trusting once the user kept the relay blocked", () => {
    view({ ...RELAY, status: { kind: "untrusted", keptBlocked: true } });
    expect(screen.getByRole("button", { name: "Trust new relay" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Keep blocked" })).toBeNull();
  });

  it("reports two announcing relays (AC-4.14)", () => {
    view({ bots: ["BotA", "BotB"], host: null, status: { kind: "conflict" }, firstSeen: null });
    expect(valueOf("Mumbot")).toBe("Yes (BotA, BotB)");
    expect(valueOf("Mumpeer relay")).toBe("Two relays are announced on this server; none is used.");
  });
});

describe("RelayRows", () => {
  it("follows relayChanged and runs the trust actions", async () => {
    const api = createMockApi({ seed: "demo" });
    setApi(api);
    render(
      <dl>
        <RelayRows serverId={2} Row={Row} />
      </dl>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Trust new relay" }));
    expect(await screen.findByText("Available · relay.example.org")).toBeInTheDocument();
    act(() => {
      api.mock.emit("relayChanged", {
        serverId: 2,
        info: { ...RELAY, status: { kind: "unavailable", reason: { kind: "relayDown" } } },
      });
    });
    expect(
      screen.getByText(
        "Unavailable: The relay is not reachable right now (reported by the relay bot).",
      ),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Forget relay" }));
    await waitFor(() => {
      expect(valueOf("Mumpeer relay")).toBe("No");
    });
  });

  it("tells the user when an action fails", async () => {
    const api = createMockApi({ seed: "ready" });
    setApi(api);
    api.mock.failNext("relayForget", "internal");
    render(
      <dl>
        <RelayRows serverId={1} Row={Row} />
      </dl>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Forget relay" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("That did not work.");
  });
});
