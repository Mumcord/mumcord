import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { subscribeAnnouncements, type Announcement } from "../../a11y/announcer";
import type { MockApi } from "../../api";
import { getServerState } from "../channels/serverStore";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { averageColor } from "./avatarColor";
import { NOTE_DEBOUNCE_MS, ProfileCard, SAVED_VISIBLE_MS, VOLUME_DEBOUNCE_MS } from "./ProfileCard";

let api: MockApi;

async function renderCard(session = 2, onSendPrivateMessage = vi.fn(), onClose = vi.fn()) {
  api = await connectedMock();
  await waitFor(() => {
    expect(getServerState().users.has(session)).toBe(true);
  });
  render(
    <ProfileCard session={session} onClose={onClose} onSendPrivateMessage={onSendPrivateMessage} />,
  );
  const dialog = await screen.findByRole("dialog");
  // The private note appears once the profile has loaded.
  if (session !== getServerState().selfSession) await screen.findByLabelText("Private note");
  return { dialog, onSendPrivateMessage, onClose };
}

const savedFlag = (dialog: HTMLElement) => dialog.querySelector(".profile-card__saved");

beforeEach(stubDialog);
afterEach(() => {
  vi.useRealTimers();
  resetAll();
});

describe("ProfileCard (full profile)", () => {
  it("shows the header: banner, avatar, name, registration and comment", async () => {
    const { dialog } = await renderCard();
    expect(screen.getByRole("heading", { level: 2, name: "Anna" })).toBeInTheDocument();
    expect(within(dialog).getByText("Registered")).toBeInTheDocument();
    expect(await within(dialog).findByText("Hi, I am Anna.")).toBeInTheDocument();
    const banner = dialog.querySelector<HTMLElement>(".profile-card__banner");
    // Without a decodable avatar (jsdom has no canvas) the banner uses the name colour.
    expect(banner?.style.getPropertyValue("--banner-bg")).toMatch(/^var\(--color-avatar-\d\)$/);
    expect(dialog.querySelector(".profile-card__header .avatar")).not.toBeNull();
  });

  it("puts Private message in the header and has no save button", async () => {
    const { dialog, onSendPrivateMessage, onClose } = await renderCard();
    const pm = within(dialog).getByRole("button", { name: "Private message" });
    expect(dialog.querySelector(".profile-card__banner")?.contains(pm)).toBe(true);
    expect(within(dialog).queryByRole("button", { name: /save/i })).toBeNull();
    fireEvent.click(pm);
    expect(onSendPrivateMessage).toHaveBeenCalledWith(expect.objectContaining({ session: 2 }));
    expect(onClose).toHaveBeenCalled();
  });

  it("applies the volume instantly and confirms with Saved, also for screen readers", async () => {
    const heard: Announcement[] = [];
    const stop = subscribeAnnouncements((a) => heard.push(a));
    const { dialog } = await renderCard();
    expect(savedFlag(dialog)).toHaveAttribute("data-visible", "false");

    vi.useFakeTimers();
    fireEvent.change(screen.getByRole("slider", { name: "Volume" }), { target: { value: "150" } });
    await act(() => vi.advanceTimersByTimeAsync(VOLUME_DEBOUNCE_MS));
    expect(getServerState().users.get(2)?.volume).toBe(150);
    expect(savedFlag(dialog)).toHaveAttribute("data-visible", "true");
    expect(heard.map((a) => a.message)).toContain("Saved");

    await act(() => vi.advanceTimersByTimeAsync(SAVED_VISIBLE_MS));
    expect(savedFlag(dialog)).toHaveAttribute("data-visible", "false");
    stop();
  });

  it("toggles local mute as a switch and shows Saved", async () => {
    const { dialog } = await renderCard();
    const mute = screen.getByRole("switch", { name: "Mute for me" });
    expect(mute).toHaveAttribute("aria-checked", "false");
    expect(mute).toHaveAccessibleDescription("You won't hear this person.");
    fireEvent.click(mute);
    await waitFor(() => {
      expect(screen.getByRole("switch", { name: "Mute for me" })).toHaveAttribute(
        "aria-checked",
        "true",
      );
    });
    expect(getServerState().users.get(2)?.localMute).toBe(true);
    expect(savedFlag(dialog)).toHaveAttribute("data-visible", "true");
  });

  it("saves the private note while typing, keeps it and confirms with Saved", async () => {
    const { dialog } = await renderCard();
    const setNote = vi.spyOn(api, "personSetNote");
    const note = screen.getByLabelText("Private note");
    expect(note).toHaveAccessibleDescription("Only you can see this note.");

    vi.useFakeTimers();
    fireEvent.change(note, { target: { value: "Plays" } });
    fireEvent.change(note, { target: { value: "Plays bass" } });
    expect(setNote).not.toHaveBeenCalled();
    await act(() => vi.advanceTimersByTimeAsync(NOTE_DEBOUNCE_MS));
    expect(setNote).toHaveBeenCalledTimes(1);
    expect(setNote).toHaveBeenCalledWith(2, "Plays bass");
    expect(savedFlag(dialog)).toHaveAttribute("data-visible", "true");
    vi.useRealTimers();

    await expect(api.personProfile(2)).resolves.toMatchObject({ note: "Plays bass" });
  });

  it("saves a pending note at once when the field loses focus", async () => {
    await renderCard();
    const setNote = vi.spyOn(api, "personSetNote");
    const note = screen.getByLabelText("Private note");
    fireEvent.change(note, { target: { value: "Friend of Ben" } });
    fireEvent.blur(note);
    expect(setNote).toHaveBeenCalledWith(2, "Friend of Ben");
  });

  it("shows the stored note when opened", async () => {
    api = await connectedMock();
    await api.personSetNote(2, "Met at the LAN party");
    render(<ProfileCard session={2} onClose={() => undefined} />);
    expect(await screen.findByLabelText("Private note")).toHaveValue("Met at the LAN party");
  });

  it("reports a failed save instead of showing Saved", async () => {
    const { dialog } = await renderCard();
    vi.spyOn(api, "personSetLocalMute").mockRejectedValue({ code: "notConnected", message: "" });
    fireEvent.click(screen.getByRole("switch", { name: "Mute for me" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Not connected to a server.");
    expect(savedFlag(dialog)).toHaveAttribute("data-visible", "false");
  });
});

describe("averageColor", () => {
  it("adds a friend from Your settings and confirms with Saved", async () => {
    const { dialog } = await renderCard();
    const add = vi.spyOn(api, "friendAdd");
    const toggle = within(dialog).getByRole("switch", { name: "Friend" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    fireEvent.click(toggle);
    expect(add).toHaveBeenCalledWith(2);
    await waitFor(() => {
      expect(within(dialog).getByRole("switch", { name: "Friend" })).toHaveAttribute(
        "aria-checked",
        "true",
      );
    });
    expect(savedFlag(dialog)).toHaveAttribute("data-visible", "true");
  });

  it("explains that people without a certificate can't be friends", async () => {
    const { dialog } = await renderCard(3);
    const add = vi.spyOn(api, "friendAdd");
    const toggle = within(dialog).getByRole("switch", { name: "Friend" });
    expect(toggle).toHaveAttribute("aria-disabled", "true");
    expect(toggle).toHaveAccessibleDescription(/without a certificate/);
    fireEvent.click(toggle);
    expect(add).not.toHaveBeenCalled();
  });

  it("averages the visible pixels and ignores transparent ones", () => {
    const pixels = new Uint8ClampedArray([255, 0, 0, 255, 0, 0, 255, 255, 9, 9, 9, 0]);
    expect(averageColor(pixels)).toBe("rgb(128 0 128)");
    expect(averageColor(new Uint8ClampedArray([1, 2, 3, 0]))).toBeNull();
  });
});
