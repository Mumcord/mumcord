import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi, NowPlaying } from "../../api";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { resetNowPlayingStore } from "../nowplaying";
import { MembersPanel } from "./MembersPanel";
import { placeNextTo } from "./placement";
import { ProfilePreviewHost } from "./ProfilePreview";
import { PREVIEW_CLOSE_GRACE_MS, PREVIEW_OPEN_DELAY_MS } from "./profilePreviewStore";

let api: MockApi;

async function renderWithPreview() {
  api = await connectedMock();
  vi.spyOn(api, "personProfile").mockResolvedValue({
    comment: '<p>Hi there</p><img src="x" onerror="alert(1)">',
    avatar: null,
  });
  render(
    <>
      <MembersPanel />
      <ProfilePreviewHost />
    </>,
  );
  await screen.findByRole("heading", { name: "In Lobby (3)" });
  vi.useFakeTimers();
}

const member = (name: RegExp) => screen.getByRole("button", { name });
const trigger = (name: RegExp) => member(name).parentElement as HTMLElement;
const preview = () => screen.queryByRole("tooltip");

/** Advances fake time and lets the profile request resolve. */
async function wait(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

async function hover(name: RegExp) {
  fireEvent.pointerEnter(trigger(name), { pointerType: "mouse" });
  await wait(PREVIEW_OPEN_DELAY_MS);
}

beforeEach(stubDialog);
afterEach(() => {
  vi.useRealTimers();
  resetNowPlayingStore();
  resetAll();
});

describe("ProfilePreview", () => {
  it("opens after the hover delay with name, registration, voice state and a sanitized comment", async () => {
    await renderWithPreview();
    act(() => {
      api.mock.setTalking(2, true);
    });
    fireEvent.pointerEnter(trigger(/^Anna/), { pointerType: "mouse" });
    await wait(PREVIEW_OPEN_DELAY_MS - 1);
    expect(preview()).toBeNull();
    await wait(1);

    const card = screen.getByRole("tooltip");
    expect(card).toHaveTextContent("Anna");
    expect(card).toHaveTextContent("Registered");
    expect(card).toHaveTextContent(/talking/i);
    expect(card).toHaveTextContent("Hi there");
    expect(card.innerHTML).not.toContain("onerror");
    expect(trigger(/^Anna/)).toHaveAttribute("aria-describedby", card.id);
  });

  it("shows what a person listens to, and the own track on the own entry", async () => {
    await renderWithPreview();
    vi.spyOn(api, "nowPlayingCover").mockResolvedValue("data:image/png;base64,AAAA");
    const snapshot = await api.sessionSnapshot();
    const own = snapshot?.users.find((u) => u.session === snapshot.selfSession);
    if (!own) throw new Error("not connected");
    const song: NowPlaying = {
      title: "Song",
      artist: "Band",
      album: null,
      hasCover: true,
      positionMs: 60_000,
      durationMs: 200_000,
      playing: true,
    };
    act(() => {
      api.mock.emit("nowPlayingChanged", { session: 2, track: song });
      api.mock.emit("nowPlayingChanged", {
        session: own.session,
        track: { ...song, title: "Own song" },
      });
    });
    // The own entry in the member list carries the music icon like everyone else's.
    expect(
      member(new RegExp(`^${own.name}`)).parentElement?.querySelector('[data-icon="music"]'),
    ).not.toBeNull();

    await hover(/^Anna/);
    let block = within(screen.getByRole("tooltip")).getByRole("region", { name: "Now playing" });
    expect(block).toHaveTextContent("Song");
    expect(block).toHaveTextContent("Band");
    expect(within(block).getByRole("progressbar")).toHaveAttribute("max", "200000");
    await wait(0);
    expect(block.querySelector("img")).toHaveAttribute("src", "data:image/png;base64,AAAA");

    fireEvent.pointerLeave(trigger(/^Anna/), { pointerType: "mouse" });
    await wait(PREVIEW_CLOSE_GRACE_MS);
    await hover(new RegExp(`^${own.name}`));
    block = within(screen.getByRole("tooltip")).getByRole("region", { name: "Now playing" });
    expect(block).toHaveTextContent("Own song");
  });

  it("does not open when the pointer leaves before the delay", async () => {
    await renderWithPreview();
    fireEvent.pointerEnter(trigger(/^Anna/), { pointerType: "mouse" });
    await wait(PREVIEW_OPEN_DELAY_MS / 2);
    fireEvent.pointerLeave(trigger(/^Anna/), { pointerType: "mouse" });
    await wait(PREVIEW_OPEN_DELAY_MS * 2);
    expect(preview()).toBeNull();
  });

  it("stays while the pointer moves into the preview and closes after leaving it", async () => {
    await renderWithPreview();
    await hover(/^Anna/);
    fireEvent.pointerLeave(trigger(/^Anna/), { pointerType: "mouse" });
    fireEvent.pointerEnter(screen.getByRole("tooltip"), { pointerType: "mouse" });
    await wait(1000);
    expect(preview()).not.toBeNull();

    fireEvent.pointerLeave(screen.getByRole("tooltip"), { pointerType: "mouse" });
    await wait(PREVIEW_CLOSE_GRACE_MS - 1);
    expect(preview()).not.toBeNull();
    await wait(1);
    expect(preview()).toBeNull();
    expect(trigger(/^Anna/)).not.toHaveAttribute("aria-describedby");
  });

  it("closes on Escape, scroll and window blur", async () => {
    await renderWithPreview();
    await hover(/^Anna/);
    fireEvent.keyDown(document.body, { key: "Escape" });
    expect(preview()).toBeNull();

    await hover(/^Zoe/);
    fireEvent.scroll(window);
    expect(preview()).toBeNull();

    await hover(/^Ben/);
    expect(screen.getByRole("tooltip")).toHaveTextContent("Not registered");
    fireEvent.blur(window);
    expect(preview()).toBeNull();
  });

  it("switches right away to the next person while one is open", async () => {
    await renderWithPreview();
    await hover(/^Anna/);
    fireEvent.pointerLeave(trigger(/^Anna/), { pointerType: "mouse" });
    fireEvent.pointerEnter(trigger(/^Ben/), { pointerType: "mouse" });
    await wait(0);
    expect(screen.getAllByRole("tooltip")).toHaveLength(1);
    expect(screen.getByRole("tooltip")).toHaveTextContent("Ben");
  });

  it("still opens the full profile card on click", async () => {
    await renderWithPreview();
    await hover(/^Anna/);
    fireEvent.pointerDown(member(/^Anna/));
    fireEvent.click(member(/^Anna/));
    await wait(0);
    expect(preview()).toBeNull();
    expect(screen.getByRole("dialog", { name: "Anna" })).toBeInTheDocument();
  });

  it("opens on keyboard focus, described by the focused person, also for listeners", async () => {
    await renderWithPreview();
    // Keyboard use, so the focus counts as :focus-visible (earlier tests used the pointer).
    fireEvent.keyDown(document.body, { key: "Tab" });
    act(() => {
      member(/^Carla/).focus();
    });
    await wait(PREVIEW_OPEN_DELAY_MS);
    const card = screen.getByRole("tooltip");
    expect(card).toHaveTextContent("Carla");
    expect(member(/^Carla/)).toHaveAttribute("aria-describedby", card.id);
    expect(member(/^Carla/)).toHaveFocus();

    fireEvent.keyDown(member(/^Carla/), { key: "Escape" });
    expect(preview()).toBeNull();
    expect(member(/^Carla/)).not.toHaveAttribute("aria-describedby");
  });

  it("ignores touch hover", async () => {
    await renderWithPreview();
    fireEvent.pointerEnter(trigger(/^Anna/), { pointerType: "touch" });
    await wait(PREVIEW_OPEN_DELAY_MS);
    expect(preview()).toBeNull();
  });
});

describe("placeNextTo", () => {
  const anchor = { left: 100, right: 300, top: 50, bottom: 80 };

  it("prefers the right side, aligned with the top", () => {
    expect(placeNextTo(anchor, 200, 100, 1200, 800)).toEqual({ left: 308, top: 50 });
  });

  it("flips to the left when the right side has no room", () => {
    const right = { left: 900, right: 1190, top: 50, bottom: 80 };
    expect(placeNextTo(right, 280, 100, 1200, 800)).toEqual({ left: 612, top: 50 });
  });

  it("goes below when neither side has room, clamped inside the window", () => {
    const wide = { left: 0, right: 1200, top: 50, bottom: 80 };
    expect(placeNextTo(wide, 280, 100, 1200, 800)).toEqual({ left: 8, top: 88 });
  });

  it("stays inside the window at the bottom edge", () => {
    const low = { left: 100, right: 300, top: 760, bottom: 790 };
    expect(placeNextTo(low, 200, 300, 1200, 800)).toEqual({ left: 308, top: 492 });
  });

  it("stays open when another container scrolls (chat following a message), closes on page scroll", async () => {
    await renderWithPreview();
    await hover(/^Anna/);
    expect(preview()).not.toBeNull();
    const other = document.createElement("div");
    document.body.append(other);
    act(() => {
      other.dispatchEvent(new Event("scroll"));
    });
    expect(preview()).not.toBeNull();
    act(() => {
      api.mock.receiveMessage({ kind: "channel", channelId: 1 }, "hello", 3);
    });
    expect(preview()).not.toBeNull();
    act(() => {
      document.dispatchEvent(new Event("scroll"));
    });
    expect(preview()).toBeNull();
    other.remove();
  });
});
