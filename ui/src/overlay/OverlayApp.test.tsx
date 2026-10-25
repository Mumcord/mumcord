import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MockApi, OverlaySettings } from "../api";
import { connectedMock, resetAll } from "../features/channels/testUtils";
import { axeViolations } from "../test/axe";
import { OverlayApp } from "./OverlayApp";
import { FADE_MS } from "./useTalkers";

let api: MockApi;

async function renderOverlay(options: Partial<OverlaySettings> = {}) {
  api = await connectedMock();
  const current = await api.overlayState();
  await api.overlayUpdate({ ...current.settings, enabled: true, ...options });
  const view = render(<OverlayApp />);
  // The options load asynchronously; flush them.
  await act(async () => {
    await Promise.resolve();
  });
  return view;
}

const list = () => screen.queryByRole("list", { name: "Talking now" });
const row = (name: string) =>
  screen.getAllByRole("listitem").find((item) => item.textContent.includes(name));

afterEach(() => {
  vi.useRealTimers();
  resetAll();
});

describe("OverlayApp", () => {
  it("draws nothing while nobody talks", async () => {
    const { container } = await renderOverlay();
    expect(container).toBeEmptyDOMElement();
  });

  it("shows talking people with the talking ring and the channel name", async () => {
    await renderOverlay();
    act(() => {
      api.mock.setTalking(2, true);
    });
    expect(list()).toBeInTheDocument();
    const anna = row("Anna");
    expect(anna).toHaveAttribute("data-talking", "true");
    expect(anna?.querySelector(".avatar")).toHaveAttribute("data-talking", "true");
    expect(screen.getByText("Lobby")).toBeInTheDocument();
    expect(screen.getAllByRole("listitem")).toHaveLength(1);
    expect(await axeViolations(document.body)).toEqual([]);
  });

  it("never lists people talking in other channels and follows a channel switch", async () => {
    await renderOverlay();
    act(() => {
      api.mock.setTalking(2, true);
      api.mock.setTalking(4, true);
    });
    expect(screen.getAllByRole("listitem").map((li) => li.textContent)).toEqual(["Anna"]);
    await act(async () => {
      await api.channelJoin(2);
    });
    expect(screen.getAllByRole("listitem").map((li) => li.textContent)).toEqual(["Carla"]);
  });

  it("fades people out after they stop talking", async () => {
    await renderOverlay();
    vi.useFakeTimers();
    act(() => {
      api.mock.setTalking(2, true);
      api.mock.setTalking(1, true);
    });
    expect(screen.getAllByRole("listitem").map((li) => li.textContent)).toEqual(["Anna", "Zoe"]);
    act(() => {
      api.mock.setTalking(2, false);
    });
    expect(row("Anna")).toHaveAttribute("data-fading", "true");
    expect(row("Anna")).toHaveAttribute("data-talking", "false");
    act(() => {
      vi.advanceTimersByTime(FADE_MS - 1);
    });
    expect(row("Anna")).toBeDefined();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(row("Anna")).toBeUndefined();
    expect(row("Zoe")).toHaveAttribute("data-fading", "false");
  });

  it("keeps someone who talks again before the fade ends", async () => {
    await renderOverlay();
    vi.useFakeTimers();
    act(() => {
      api.mock.setTalking(2, true);
    });
    act(() => {
      api.mock.setTalking(2, false);
    });
    act(() => {
      vi.advanceTimersByTime(FADE_MS / 2);
      api.mock.setTalking(2, true);
    });
    act(() => {
      vi.advanceTimersByTime(FADE_MS);
    });
    expect(row("Anna")).toHaveAttribute("data-fading", "false");
  });

  it("respects the options: whole channel, no channel name, opacity", async () => {
    await renderOverlay({ showWholeChannel: true, showChannelName: false, opacity: 50 });
    const names = screen.getAllByRole("listitem").map((li) => li.textContent);
    expect(names).toContain("Zoe");
    expect(names).toContain("Anna");
    expect(screen.queryByText("Lobby")).not.toBeInTheDocument();
    expect(list()?.parentElement).toHaveStyle({ opacity: "0.5" });
    act(() => {
      api.mock.setTalking(2, true);
    });
    expect(row("Anna")).toHaveAttribute("data-talking", "true");
    expect(row("Zoe")).toHaveAttribute("data-talking", "false");
  });

  it("follows option changes live", async () => {
    await renderOverlay();
    act(() => {
      api.mock.setTalking(2, true);
    });
    expect(screen.getByText("Lobby")).toBeInTheDocument();
    const { settings } = await api.overlayState();
    await act(async () => {
      await api.overlayUpdate({ ...settings, showChannelName: false });
    });
    expect(screen.queryByText("Lobby")).not.toBeInTheDocument();
  });

  it("shows a drag handle in move mode and leaves it with Done or Escape", async () => {
    const { container } = await renderOverlay();
    await act(async () => {
      await api.overlayMoveMode(true);
    });
    expect(container.querySelector("[data-tauri-drag-region]")).not.toBeNull();
    expect(screen.getByText("Drag to move the overlay")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    await act(async () => {
      await Promise.resolve();
    });
    expect(container).toBeEmptyDOMElement();

    await act(async () => {
      await api.overlayMoveMode(true);
    });
    fireEvent.keyDown(window, { key: "Escape" });
    await act(async () => {
      await Promise.resolve();
    });
    expect((await api.overlayState()).moveMode).toBe(false);
  });
});
