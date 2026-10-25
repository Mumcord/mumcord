import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PreferencesProvider } from "../../app/PreferencesProvider";
import { ViewContext, type View } from "../../app/view";
import { createMockApi, setApi, type MockApi, type Settings } from "../../api";
import { subscribeAnnouncements } from "../../a11y/announcer";
import "../../i18n";
import { axeViolations } from "../../test/axe";
import { resetServerStore } from "../channels/serverStore";
import { resetNowPlayingStore } from "../nowplaying";
import { UserPanel } from "./UserPanel";
import { LEVEL_FRAME_MS } from "./useMicLevelFill";

const files = vi.hoisted(() => ({
  pickImagePath: vi.fn<() => Promise<string | null>>(),
  pickSavePath: vi.fn<() => Promise<string | null>>(),
}));
vi.mock("./files", () => files);

const openSettings = vi.fn();
const view: View = {
  screen: "main",
  overlay: null,
  finishWizard: vi.fn(),
  openSettings,
  closeOverlay: vi.fn(),
} as unknown as View;

let api: MockApi;
let announcements: string[];
let stopAnnouncements: () => void;

async function setup(connect = true, registered = false, talk?: Partial<Settings>) {
  api = createMockApi({ seed: "ready" });
  setApi(api);
  if (talk) await api.settingsUpdate({ ...(await api.settingsGet()), ...talk });
  resetServerStore();
  if (connect) {
    const [server] = await api.serversList();
    await api.connect((server ?? { id: 0 }).id, null);
  }
  render(
    <PreferencesProvider>
      <ViewContext.Provider value={view}>
        <UserPanel />
      </ViewContext.Provider>
    </PreferencesProvider>,
  );
  if (connect) {
    await screen.findByRole("button", { name: "Mute microphone" });
    const snapshot = await api.sessionSnapshot();
    const self = snapshot?.users.find((u) => u.session === snapshot.selfSession);
    if (self)
      act(() => {
        api.mock.emit("userUpserted", { user: { ...self, registered } });
      });
  }
}

beforeEach(() => {
  announcements = [];
  stopAnnouncements = subscribeAnnouncements((a) => announcements.push(a.message));
  files.pickImagePath.mockReset();
  files.pickSavePath.mockReset();
});

afterEach(() => {
  stopAnnouncements();
  setApi(undefined);
  resetServerStore();
  resetNowPlayingStore();
});

const mute = () => screen.getByRole("button", { name: "Mute microphone" });
const deafen = () => screen.getByRole("button", { name: "Deafen" });
const READ_ONLY = /reading along/;

/** Leaves the read-only start of a fresh connect through the notice's join button. */
async function joinVoice() {
  fireEvent.click(await screen.findByRole("button", { name: /^Join / }));
  await waitFor(() => {
    expect(screen.queryByText(READ_ONLY)).not.toBeInTheDocument();
  });
}

/** Disconnects and connects again: a fresh, user-initiated connect. */
async function freshConnect() {
  const [server] = await api.serversList();
  await act(() => api.disconnect());
  await act(() => api.connect((server ?? { id: 0 }).id, null));
  await screen.findByRole("button", { name: "Mute microphone" });
}

async function openMenuItem(name: string) {
  const opener = document.querySelector('[aria-haspopup="menu"]');
  if (!opener) throw new Error("user menu button missing");
  fireEvent.click(opener);
  fireEvent.click(await screen.findByRole("menuitem", { name }));
}

describe("UserPanel", () => {
  it("offers a named disconnect button only while connected and calls disconnect", async () => {
    await setup();
    const button = await screen.findByRole("button", { name: /^Disconnect from / });
    expect(button).toHaveAttribute("title", button.getAttribute("aria-label"));
    const spy = vi.spyOn(api, "disconnect");
    fireEvent.click(button);
    await waitFor(() => {
      expect(spy).toHaveBeenCalled();
    });
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: /^Disconnect from / })).not.toBeInTheDocument();
    });
  });

  it("hides the disconnect button while disconnected", async () => {
    await setup(false);
    expect(screen.queryByRole("button", { name: /^Disconnect from / })).not.toBeInTheDocument();
  });

  it("shows a notice when the default avatar was too large for the server", async () => {
    await setup();
    act(() => {
      api.mock.emit("defaultAvatarSkipped", { serverId: 1 });
    });
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Your default avatar was not sent: it is too large for this server.",
    );
  });

  it("toggles mute and reflects it", async () => {
    await setup();
    await joinVoice();
    expect(mute()).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(mute());
    await waitFor(() => {
      expect(mute()).toHaveAttribute("aria-pressed", "true");
    });
    expect(announcements).toContain("Microphone muted");
  });

  it("deafen implies mute and undeafen restores it", async () => {
    await setup();
    await joinVoice();
    fireEvent.click(deafen());
    await waitFor(() => {
      expect(deafen()).toHaveAttribute("aria-pressed", "true");
    });
    expect(mute()).toHaveAttribute("aria-pressed", "true");
    expect(announcements.join("|")).toContain("Deafened");
    fireEvent.click(deafen());
    await waitFor(() => {
      expect(deafen()).toHaveAttribute("aria-pressed", "false");
    });
    expect(mute()).toHaveAttribute("aria-pressed", "false");
  });

  it("unmuting while deafened undeafens", async () => {
    await setup();
    await joinVoice();
    fireEvent.click(deafen());
    await waitFor(() => {
      expect(deafen()).toHaveAttribute("aria-pressed", "true");
    });
    fireEvent.click(mute());
    await waitFor(() => {
      expect(deafen()).toHaveAttribute("aria-pressed", "false");
    });
    expect(mute()).toHaveAttribute("aria-pressed", "false");
  });

  it("starts read-only after a fresh connect; the notice's join button turns voice on", async () => {
    await setup();
    expect(screen.getByText(READ_ONLY)).toBeInTheDocument();
    expect(mute()).toHaveAttribute("aria-pressed", "true");
    expect(deafen()).toHaveAttribute("aria-pressed", "true");
    const spy = vi.spyOn(api, "channelJoin");
    const snapshot = await api.sessionSnapshot();
    const self = snapshot?.users.find((u) => u.session === snapshot.selfSession);
    await joinVoice();
    expect(spy).toHaveBeenCalledWith(self?.channelId);
    expect(mute()).toHaveAttribute("aria-pressed", "false");
    expect(deafen()).toHaveAttribute("aria-pressed", "false");
  });

  it("a join after a fresh connect restores the last own mute choice (QA.7)", async () => {
    await setup();
    await joinVoice();
    fireEvent.click(mute());
    await waitFor(() => {
      expect(mute()).toHaveAttribute("aria-pressed", "true");
    });
    await freshConnect();
    expect(await screen.findByText(READ_ONLY)).toBeInTheDocument();
    await joinVoice();
    expect(mute()).toHaveAttribute("aria-pressed", "true");
    expect(deafen()).toHaveAttribute("aria-pressed", "false");
  });

  it("a join restoring a deafened choice still ends read-only", async () => {
    await setup();
    await joinVoice();
    fireEvent.click(deafen());
    await waitFor(() => {
      expect(deafen()).toHaveAttribute("aria-pressed", "true");
    });
    await freshConnect();
    expect(await screen.findByText(READ_ONLY)).toBeInTheDocument();
    await joinVoice();
    expect(deafen()).toHaveAttribute("aria-pressed", "true");
    expect(mute()).toHaveAttribute("aria-pressed", "true");
  });

  it("undeafening ends read-only; a deliberate deafen later shows no notice", async () => {
    await setup();
    fireEvent.click(deafen());
    await waitFor(() => {
      expect(screen.queryByText(READ_ONLY)).not.toBeInTheDocument();
    });
    fireEvent.click(deafen());
    await waitFor(() => {
      expect(deafen()).toHaveAttribute("aria-pressed", "true");
    });
    expect(screen.queryByText(READ_ONLY)).not.toBeInTheDocument();
  });

  it("an automatic reconnect keeps the read-only state as it was", async () => {
    await setup();
    act(() => {
      api.mock.loseConnection();
      api.mock.completeReconnect();
    });
    expect(screen.getByText(READ_ONLY)).toBeInTheDocument();
    await joinVoice();
    fireEvent.click(deafen());
    await waitFor(() => {
      expect(deafen()).toHaveAttribute("aria-pressed", "true");
    });
    act(() => {
      api.mock.loseConnection();
      api.mock.completeReconnect();
    });
    expect(screen.queryByText(READ_ONLY)).not.toBeInTheDocument();
  });

  it("offers connection info only while connected", async () => {
    await setup();
    fireEvent.click(screen.getByRole("button", { name: "Connection info" }));
    expect(await screen.findByRole("dialog", { name: "Connection info" })).toBeInTheDocument();
  });

  it("hides connection info when not connected", async () => {
    await setup(false);
    expect(screen.queryByRole("button", { name: "Connection info" })).not.toBeInTheDocument();
  });

  it("opens settings", async () => {
    await setup();
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(openSettings).toHaveBeenCalled();
  });

  it("disables toggles when not connected", async () => {
    await setup(false);
    expect(mute()).toBeDisabled();
    expect(deafen()).toBeDisabled();
  });

  it("registers after explaining and exporting the identity, then hides the menu item", async () => {
    await setup();
    await api.identityCreate("Self");
    const exportSpy = vi.spyOn(api, "identityExport");
    files.pickSavePath.mockResolvedValue("/tmp/id.p12");
    await openMenuItem("Register on this server");
    const dialog = await screen.findByRole("dialog", { name: "Register on this server" });
    expect(dialog).toHaveTextContent(/back up|backup/i);
    fireEvent.click(screen.getByRole("button", { name: "Export identity" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Enter a password");
    fireEvent.change(screen.getByLabelText("Password for the backup file"), {
      target: { value: "pw" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Export identity" }));
    await waitFor(() => {
      expect(exportSpy).toHaveBeenCalledWith("/tmp/id.p12", "pw");
    });
    fireEvent.click(screen.getByRole("button", { name: "Register" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    const opener = document.querySelector('[aria-haspopup="menu"]');
    if (!opener) throw new Error("user menu button missing");
    fireEvent.click(opener);
    await screen.findByRole("menuitem", { name: "Edit profile" });
    expect(screen.queryByRole("menuitem", { name: "Register on this server" })).toBeNull();
  });

  it("shows a plain error when registering is refused", async () => {
    await setup();
    await openMenuItem("Register on this server");
    api.mock.failNext("registerSelf", "permissionDenied", "No way");
    fireEvent.click(await screen.findByRole("button", { name: "Register" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("No way");
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("saves avatar and comment", async () => {
    await setup();
    const avatar = vi.spyOn(api, "setOwnAvatar");
    const comment = vi.spyOn(api, "setOwnComment");
    files.pickImagePath.mockResolvedValue("/home/me/face.png");
    await openMenuItem("Edit profile");
    fireEvent.click(await screen.findByRole("button", { name: "Choose picture…" }));
    await screen.findByText("Selected: face.png");
    const text = screen.getByLabelText("About you");
    await waitFor(() => {
      expect(text).toBeEnabled();
    });
    fireEvent.change(text, { target: { value: "a < b\nline" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(avatar).toHaveBeenCalledWith("/home/me/face.png");
    expect(comment).toHaveBeenCalledWith("a &lt; b<br>line");
  });

  it("explains the avatar re-send only to unregistered users", async () => {
    const hint = /keeps avatars only for registered users/;
    await setup();
    await openMenuItem("Edit profile");
    expect(await screen.findByRole("dialog")).toHaveTextContent(hint);
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await act(async () => {
      await api.registerSelf();
    });
    await openMenuItem("Edit profile");
    expect(await screen.findByRole("dialog")).not.toHaveTextContent(hint);
  });

  it("shows the size limit error and keeps the dialog open", async () => {
    await setup();
    await openMenuItem("Edit profile");
    const text = screen.getByLabelText("About you");
    await waitFor(() => {
      expect(text).toBeEnabled();
    });
    fireEvent.change(text, { target: { value: "x" } });
    api.mock.failNext("setOwnComment", "messageTooLarge");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("too large for the server");
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("shows the own avatar when the server has one and refreshes on a hash change", async () => {
    await setup();
    const userButton = () => {
      const el = document.querySelector<HTMLElement>('[aria-haspopup="menu"]');
      if (!el) throw new Error("user button missing");
      return el;
    };
    expect(userButton().querySelector("img")).toBeNull();
    expect(userButton().querySelector(".userpanel-avatar")).toHaveAttribute("data-initial");
    const personProfile = vi.spyOn(api, "personProfile");
    await act(async () => {
      await api.setOwnAvatar("/a.png");
    });
    await waitFor(() => {
      expect(userButton().querySelector("img")).not.toBeNull();
    });
    expect(personProfile).toHaveBeenCalledTimes(1);
    await act(async () => {
      await api.setOwnAvatar("/b.png");
    });
    await waitFor(() => {
      expect(personProfile).toHaveBeenCalledTimes(2);
    });
    await act(async () => {
      await api.setOwnAvatar(null);
    });
    await waitFor(() => {
      expect(userButton().querySelector("img")).toBeNull();
    });
  });

  it("shows the name with a status line in the top row and the buttons below", async () => {
    await setup();
    const name = document.querySelector<HTMLElement>(".userpanel__name");
    if (!name) throw new Error("name missing");
    expect(name).toHaveAttribute("title", name.textContent);
    const button = document.querySelector<HTMLElement>('[aria-haspopup="menu"]');
    // The visible name and status line are the button's accessible name (WCAG 2.5.3).
    expect(button).toHaveAccessibleName(`${name.textContent} Read-only`);
    expect(button?.closest(".userpanel__identity")).not.toBeNull();
    const row = document.querySelector(".userpanel__row");
    if (!row) throw new Error("button row missing");
    expect(row.contains(button)).toBe(false);
    const labels = [...row.querySelectorAll("button")].map((b) => b.getAttribute("aria-label"));
    expect(labels).toEqual([
      "Mute microphone",
      "Deafen",
      expect.stringMatching(/^Disconnect from /),
      "Connection info",
      "Settings",
    ]);
    await joinVoice();
    expect(button).not.toHaveAccessibleDescription("Read-only");
    expect(button?.querySelector(".userpanel__status")?.textContent).not.toBe("");
  });

  it("shows the own playing track in the status line, with the full text as tooltip", async () => {
    await setup();
    await joinVoice();
    const status = () => document.querySelector<HTMLElement>(".userpanel__status");
    const normal = status()?.textContent ?? "";
    const self = (await api.sessionSnapshot())?.selfSession ?? -1;
    const track = {
      title: "A very long song title",
      artist: "Band",
      album: null,
      hasCover: false,
      positionMs: 1000,
      durationMs: 200_000,
      playing: true,
    };
    act(() => {
      api.mock.emit("nowPlayingChanged", { session: self, track });
    });
    expect(status()).toHaveTextContent("Band – A very long song title");
    expect(status()).toHaveAttribute("title", "Band – A very long song title");
    expect(status()?.querySelector("svg.userpanel__status-icon")).not.toBeNull();
    // Paused or stopped: the usual status line again.
    act(() => {
      api.mock.emit("nowPlayingChanged", { session: self, track: { ...track, playing: false } });
    });
    expect(status()?.textContent).toBe(normal);
    expect(status()?.querySelector("svg")).toBeNull();
    act(() => {
      api.mock.emit("nowPlayingChanged", { session: self, track });
    });
    act(() => {
      api.mock.emit("nowPlayingChanged", { session: self, track: null });
    });
    expect(status()?.textContent).toBe(normal);
  });

  it("keeps important states over the own track", async () => {
    await setup();
    const self = (await api.sessionSnapshot())?.selfSession ?? -1;
    act(() => {
      api.mock.emit("nowPlayingChanged", {
        session: self,
        track: {
          title: "Song",
          artist: "",
          album: null,
          hasCover: false,
          positionMs: 0,
          durationMs: null,
          playing: true,
        },
      });
    });
    expect(document.querySelector(".userpanel__status")).toHaveTextContent("Read-only");
  });

  it("lets a long name wrap to two lines instead of cutting it off", async () => {
    await setup();
    const snapshot = await api.sessionSnapshot();
    const self = snapshot?.users.find((u) => u.session === snapshot.selfSession);
    if (!self) throw new Error("self missing");
    const long = "Maximilian Schmi";
    act(() => {
      api.mock.emit("userUpserted", { user: { ...self, name: long } });
    });
    const name = await screen.findByText(long);
    expect(name).toHaveClass("userpanel__name");
    expect(name).toHaveAttribute("title", long);
    // jsdom does not apply the stylesheet, so assert the rules themselves.
    const css = readFileSync(
      resolve(process.cwd(), "ui/src/features/userpanel/userpanel.css"),
      "utf8",
    );
    const rule = (selector: string) =>
      new RegExp(`^${selector.replace(".", "\\.")} \\{([^}]*)\\}`, "m").exec(css)?.[1] ?? "";
    expect(rule(".userpanel__name")).toMatch(/-webkit-line-clamp: 2;/);
    expect(rule(".userpanel__name")).toMatch(/overflow-wrap: anywhere;/);
    expect(rule(".userpanel__name")).not.toMatch(/nowrap/);
    expect(rule(".userpanel__text")).toMatch(/min-width: 0;/);
    expect(rule(".userpanel__toggle")).toMatch(/flex: none;/);
    expect(rule(".userpanel")).not.toMatch(/border-top/);
  });

  it("fills the mic button with the live input level, throttled, and hides it while muted", async () => {
    await setup();
    await joinVoice();
    vi.useFakeTimers();
    try {
      const fill = () => mute().querySelector(".userpanel__level");
      expect(fill()).not.toBeNull();
      act(() => {
        api.mock.emit("micLevel", { level: 0.8 });
        api.mock.emit("micLevel", { level: 0.6 });
        vi.advanceTimersByTime(LEVEL_FRAME_MS);
      });
      // Several events within one frame give a single paint with the newest level.
      expect(mute().style.getPropertyValue("--mic-level")).toBe("0.600");
      expect(mute()).toHaveAttribute("data-active");
      act(() => {
        for (let i = 0; i < 20; i++) {
          api.mock.emit("micLevel", { level: 0 });
          vi.advanceTimersByTime(LEVEL_FRAME_MS);
        }
      });
      expect(mute().style.getPropertyValue("--mic-level")).toBe("0.000");
      expect(mute()).not.toHaveAttribute("data-active");
    } finally {
      vi.useRealTimers();
    }
    fireEvent.click(mute());
    await waitFor(() => {
      expect(mute()).toHaveAttribute("aria-pressed", "true");
    });
    expect(mute().querySelector(".userpanel__level")).toBeNull();
    act(() => {
      api.mock.emit("micLevel", { level: 0.9 });
    });
    await new Promise((r) => setTimeout(r, LEVEL_FRAME_MS + 10));
    expect(mute().style.getPropertyValue("--mic-level")).toBe("");
  });

  it("has no accessibility violations (panel, menu and dialog)", async () => {
    await setup();
    const { container } = { container: document.body };
    expect(await axeViolations(container)).toEqual([]);
    await openMenuItem("Edit profile");
    await act(async () => {
      await Promise.resolve();
    });
    expect(await axeViolations(container)).toEqual([]);
  });

  describe("push-to-talk", () => {
    const pttSettings: Partial<Settings> = { talkMode: "pushToTalk", pttKey: "Ctrl+KeyA" };
    const mic = () => screen.getByRole("button", { name: "Mute microphone" });
    const crossed = () => mic().querySelectorAll("svg").length === 2;
    const selfSession = async () => (await api.sessionSnapshot())?.selfSession ?? -1;

    it("shows a crossed-out mic without level while the key is not held", async () => {
      await setup(true, false, pttSettings);
      await joinVoice();
      await waitFor(() => {
        expect(mic()).toHaveAttribute("title", "Push-to-talk: hold Ctrl+A to talk");
      });
      expect(crossed()).toBe(true);
      expect(mic()).toHaveAttribute("aria-pressed", "false");
      expect(mic()).toHaveAttribute("data-ptt-idle");
      expect(mic().querySelector(".userpanel__level")).toBeNull();
    });

    it("shows the normal mic with level while the key is held", async () => {
      await setup(true, false, pttSettings);
      await joinVoice();
      await waitFor(() => {
        expect(mic()).toHaveAttribute("data-ptt-idle");
      });
      const session = await selfSession();
      act(() => {
        api.mock.emit("talkStateChanged", { session, talking: true });
      });
      expect(crossed()).toBe(false);
      expect(mic()).not.toHaveAttribute("data-ptt-idle");
      expect(mic().querySelector(".userpanel__level")).not.toBeNull();
      act(() => {
        api.mock.emit("talkStateChanged", { session, talking: false });
      });
      expect(crossed()).toBe(true);
    });

    it("keeps the muted look distinct from the idle look", async () => {
      await setup(true, false, pttSettings);
      await joinVoice();
      await waitFor(() => {
        expect(mic()).toHaveAttribute("data-ptt-idle");
      });
      fireEvent.click(mic());
      await waitFor(() => {
        expect(mic()).toHaveAttribute("aria-pressed", "true");
      });
      expect(crossed()).toBe(true);
      expect(mic()).not.toHaveAttribute("data-ptt-idle");
    });

    it("leaves voice activation unchanged", async () => {
      await setup();
      await joinVoice();
      expect(crossed()).toBe(false);
      expect(mic()).not.toHaveAttribute("data-ptt-idle");
      expect(mic()).not.toHaveAttribute("title");
      expect(mic().querySelector(".userpanel__level")).not.toBeNull();
    });
  });
});

describe("quick settings (FR-1.66)", () => {
  const gear = () => screen.getByRole("button", { name: "Settings" });

  it("opens on right-click and saves a switch right away", async () => {
    await setup();
    fireEvent.contextMenu(gear(), { clientX: 10, clientY: 10 });
    const links = await screen.findByRole("menuitemcheckbox", { name: "Link previews and embeds" });
    expect(links).toHaveAttribute("aria-checked", "true");
    fireEvent.click(links);
    await waitFor(async () => {
      expect((await api.settingsGet()).linkPreviews).toBe(false);
    });
    expect(links).toHaveAttribute("aria-checked", "false");
    expect(openSettings).not.toHaveBeenCalled();
  });

  it("opens with Shift+F10, applies the theme at once and Escape returns focus", async () => {
    await setup();
    gear().focus();
    fireEvent.keyDown(gear(), { key: "F10", shiftKey: true });
    const dark = await screen.findByRole("menuitemradio", { name: "Theme: dark" });
    fireEvent.click(dark);
    await waitFor(() => {
      expect(document.documentElement.dataset.theme).toBe("dark");
    });
    await waitFor(() => {
      expect(dark).toHaveAttribute("aria-checked", "true");
    });
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    expect(screen.queryByRole("menu", { name: "Quick settings" })).toBeNull();
    expect(gear()).toHaveFocus();
  });

  it("toggles now playing and leaves a normal click on the full settings", async () => {
    await setup();
    fireEvent.contextMenu(gear());
    fireEvent.click(
      await screen.findByRole("menuitemcheckbox", { name: "Share what I'm listening to" }),
    );
    await waitFor(async () => {
      expect((await api.settingsGet()).nowPlaying).toBe(true);
    });
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    fireEvent.click(gear());
    expect(openSettings).toHaveBeenCalled();
  });
});
