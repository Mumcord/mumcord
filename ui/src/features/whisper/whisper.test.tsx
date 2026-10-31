import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { MockApi, WhisperTarget } from "../../api";
import { connectedMock, resetAll } from "../channels/testUtils";
import { WhisperBadge } from "./WhisperBadge";
import { WhisperBanner } from "./WhisperBanner";
import { WhisperButton } from "./WhisperPanel";
import { WhisperTargetsGroup } from "./WhisperTargetsGroup";
import {
  createWhisperTarget,
  resetWhisperStore,
  saveWhisperTargets,
  startWhisper,
} from "./whisperStore";

let api: MockApi;

const raid: WhisperTarget = {
  id: "raid",
  name: "Raid Night + sub-channels",
  receivers: { kind: "channel", path: ["Raid"], children: true, links: false, group: null },
  hotkey: "F9",
};
const anna: WhisperTarget = {
  id: "anna",
  name: "Anna",
  receivers: { kind: "people", names: ["Anna"] },
  hotkey: null,
};

beforeEach(async () => {
  resetWhisperStore();
  api = await connectedMock({ bindStore: true });
});

afterEach(() => {
  resetWhisperStore();
  resetAll();
});

/** The mock's server 1 carries voice, as after joining a channel there. */
function voiceOn(serverId: number | null = 1) {
  act(() => {
    api.mock.emit("voiceServerChanged", { serverId });
  });
}

describe("incoming whisper marks (issue #67)", () => {
  it("marks a person who whispers or shouts to the user, until they stop", () => {
    render(<WhisperBadge session={7} self={false} />);
    voiceOn();
    act(() => {
      api.mock.emit("incomingWhisperChanged", { serverId: 1, session: 7, kind: "whisper" });
    });
    const badge = screen.getByRole("img", { name: "Whispers to you" });
    expect(badge).toHaveTextContent("whispers");
    act(() => {
      api.mock.emit("incomingWhisperChanged", { serverId: 1, session: 7, kind: "shout" });
    });
    expect(screen.getByRole("img", { name: "Shouts to your channel" })).toHaveTextContent("shouts");
    act(() => {
      api.mock.emit("incomingWhisperChanged", { serverId: 1, session: 7, kind: null });
    });
    expect(screen.queryByRole("img")).toBeNull();
  });

  it("ignores marks of other servers and drops them when the voice server changes", () => {
    render(<WhisperBadge session={7} self={false} />);
    voiceOn();
    act(() => {
      api.mock.emit("incomingWhisperChanged", { serverId: 2, session: 7, kind: "whisper" });
    });
    expect(screen.queryByRole("img")).toBeNull();
    act(() => {
      api.mock.emit("incomingWhisperChanged", { serverId: 1, session: 7, kind: "whisper" });
    });
    expect(screen.getByRole("img")).toBeInTheDocument();
    act(() => {
      api.mock.emit("userRemoved", { serverId: 1, session: 7 });
    });
    expect(screen.queryByRole("img")).toBeNull();
  });
});

describe("own whisper (issue #67)", () => {
  it("shows the banner, the panel chip and the own badge, and stops from the banner", async () => {
    await saveWhisperTargets([raid, anna]);
    render(
      <>
        <WhisperBanner />
        <WhisperButton className="chip" iconClassName="chip--icon" />
        <WhisperBadge session={1} self />
      </>,
    );
    await act(async () => {
      await startWhisper("raid");
    });
    expect(screen.getByText("Shouting to Raid Night + sub-channels")).toBeInTheDocument();
    const chip = screen.getByRole("button", {
      name: "Shouting to Raid Night + sub-channels · Stop",
    });
    expect(chip).toHaveTextContent("shouting");
    expect(chip).not.toHaveClass("chip--icon");
    expect(screen.getByRole("img", { name: "You are shouting" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    await waitFor(() => {
      expect(screen.queryByText(/Shouting to/)).toBeNull();
    });
    expect(screen.queryByRole("img")).toBeNull();
    expect(screen.getByRole("button", { name: "Whisper or shout" })).toHaveClass("chip--icon");
  });

  it("names the release key only for a whisper held by its key", async () => {
    await saveWhisperTargets([raid]);
    render(<WhisperBanner />);
    voiceOn();
    act(() => {
      api.mock.emit("whisperStateChanged", {
        state: { state: "active", targetId: "raid", held: true },
      });
    });
    const banner = screen.getByRole("status");
    expect(within(banner).getByText("F9").tagName).toBe("KBD");
    act(() => {
      api.mock.emit("whisperStateChanged", {
        state: { state: "active", targetId: "raid", held: false },
      });
    });
    expect(within(screen.getByRole("status")).queryByText("F9")).toBeNull();
    expect(screen.getByRole("status")).toHaveTextContent(/Stop to talk to .+ again/);
  });

  it("explains a refused whisper", async () => {
    await saveWhisperTargets([raid]);
    render(<WhisperBanner />);
    act(() => {
      api.mock.emit("whisperStateChanged", {
        state: { state: "denied", targetId: "raid", reason: "noPermission", channel: "Raid" },
      });
    });
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Could not whisper to Raid Night + sub-channels");
    expect(alert).toHaveTextContent("The server does not allow you to whisper to Raid.");
    fireEvent.click(within(alert).getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("starts a target from the user panel menu", async () => {
    await saveWhisperTargets([raid, anna]);
    render(<WhisperButton className="chip" iconClassName="chip--icon" />);
    fireEvent.click(screen.getByRole("button", { name: "Whisper or shout" }));
    const menu = screen.getByRole("menu", { name: "Whisper and shout targets" });
    fireEvent.click(within(menu).getByRole("menuitem", { name: /^Anna/ }));
    const stop = await screen.findByRole("button", { name: "Whispering to Anna · Stop" });
    expect(stop).toHaveTextContent("whispering");
    fireEvent.click(stop);
    expect(await screen.findByRole("button", { name: "Whisper or shout" })).toBeInTheDocument();
  });
});

describe("whisper targets in Settings (issue #67)", () => {
  it("creates a target from a person menu draft and saves it", async () => {
    render(<WhisperTargetsGroup />);
    act(() => {
      createWhisperTarget({ name: "Anna", receivers: { kind: "people", names: ["Anna"] } });
    });
    const name = await screen.findByPlaceholderText("Name");
    expect(name).toHaveValue("Anna");
    expect(screen.getByRole("radio", { name: "People" })).toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Saved.")).toBeInTheDocument();
    const saved = await api.whisperSetup();
    expect(saved.targets).toEqual([
      expect.objectContaining({ name: "Anna", receivers: { kind: "people", names: ["Anna"] } }),
    ]);
    expect(screen.getByRole("button", { name: /Anna.*1 person · whisper.*Not set/ })).toBeVisible();
  });

  it("captures a hold-to-talk key and refuses one already in use", async () => {
    await saveWhisperTargets([raid, anna]);
    render(<WhisperTargetsGroup />);
    fireEvent.click(await screen.findByRole("button", { name: /^Anna/ }));
    const set = screen.getByRole("button", { name: "Set key" });
    fireEvent.click(set);
    fireEvent.keyDown(set, { key: "F9", code: "F9" });
    expect(await screen.findByText(/already push-to-talk/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Set key" }));
    fireEvent.keyDown(screen.getByRole("button", { name: "Press a key…" }), {
      key: "F10",
      code: "F10",
    });
    expect(await screen.findByRole("button", { name: "Change" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText("Saved.");
    const saved = await api.whisperSetup();
    expect(saved.targets.find((target) => target.id === "anna")?.hotkey).toBe("F10");
  });

  it("explains why keys are unavailable instead of offering capture", async () => {
    api.mock.setWhisperKeysUnavailable(true);
    await saveWhisperTargets([raid]);
    render(<WhisperTargetsGroup />);
    expect(await screen.findByText(/not available in this Wayland session/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Change" })).toBeNull();
  });
});
