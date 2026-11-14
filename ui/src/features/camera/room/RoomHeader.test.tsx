/**
 * The camera room's header path chip (FR-8.62, FR-8.94): "Relay · end-to-end encrypted", "Direct ·
 * end-to-end encrypted" or "Relay and direct · end-to-end encrypted" with the sentence that says
 * what the path means — reachable with the keyboard and read out by screen readers, not only by the
 * pointer (#803).
 */
import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { CameraView } from "../../../api";
import { TOOLTIP_DELAY_MS, TooltipLayer } from "../../../components/Tooltip";
import "../../../i18n";
import { RoomHeader } from "./RoomHeader";
import type { RoomModel } from "./useRoomModel";

function onAir(broadcast: string | null, viewers: CameraView["viewers"]): CameraView {
  return { serverId: 1, state: "onAir", error: null, broadcast, viewers };
}

function model(own: CameraView): RoomModel {
  return {
    serverId: 1,
    cameraCount: 1,
    peopleCount: 1,
    channelName: "Lobby",
    tiles: [],
    speaker: null,
    large: null,
    layout: "grid",
    selfHidden: false,
    own,
    withoutCamera: [],
  };
}

/** The chip that carries `label`; the header shows exactly one of them. */
function chip(label: string): HTMLElement {
  return screen.getByText(label, { selector: ".cam-chip" });
}

const RELAY_TIP = "Every camera is sent once to the relay and copied to viewers";
const DIRECT_TIP = "Every viewer gets their own copy straight from you, without a relay";
const BOTH_TIP =
  "Viewers on older Mumcord get a copy from the relay, newer ones their own stream from you";

const CASES = [
  {
    path: "relay",
    own: onAir("00112233aabbccdd", [{ session: 2, path: "relay" }]),
    label: "Relay · end-to-end encrypted",
    tip: RELAY_TIP,
  },
  {
    path: "direct",
    own: onAir(null, [{ session: 2, path: "direct" }]),
    label: "Direct · end-to-end encrypted",
    tip: DIRECT_TIP,
  },
  {
    path: "both",
    own: onAir("00112233aabbccdd", [
      { session: 2, path: "relay" },
      { session: 3, path: "direct" },
    ]),
    label: "Relay and direct · end-to-end encrypted",
    tip: BOTH_TIP,
  },
] as const;

describe("camera room header path chip", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it.each(CASES)("$path: is a tab stop that describes the path", ({ own, label, tip }) => {
    render(<RoomHeader model={model(own)} />);
    const element = chip(label);
    expect(element).toHaveAttribute("tabindex", "0");
    act(() => {
      element.focus();
    });
    expect(element).toHaveFocus();
    expect(element).toHaveAccessibleDescription(tip);
  });

  it.each(CASES)("$path: keeps the explanation out of the visible text", ({ own, label, tip }) => {
    render(<RoomHeader model={model(own)} />);
    const element = chip(label);
    const described = (element.getAttribute("aria-describedby") ?? "").split(" ");
    const hint = described.map((id) => document.getElementById(id));
    expect(hint).toHaveLength(1);
    expect(hint[0]).toHaveClass("visually-hidden");
    expect(hint[0]).toHaveTextContent(tip);
  });

  it("opens the tooltip when the chip gets keyboard focus", () => {
    vi.useFakeTimers();
    render(
      <>
        <TooltipLayer />
        <RoomHeader model={model(CASES[0].own)} />
      </>,
    );
    act(() => {
      chip(CASES[0].label).focus();
    });
    act(() => {
      vi.advanceTimersByTime(TOOLTIP_DELAY_MS);
    });
    const tip = screen.getByRole("tooltip");
    expect(tip).toHaveTextContent(RELAY_TIP);
    // The description is the hidden text, not the tooltip: it stays there while the tip is away.
    expect(chip(CASES[0].label)).toHaveAccessibleDescription(RELAY_TIP);
  });
});
