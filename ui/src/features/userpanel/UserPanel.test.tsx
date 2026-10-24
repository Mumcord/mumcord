import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ViewContext, type View } from "../../app/view";
import { createMockApi, setApi, type MockApi } from "../../api";
import { subscribeAnnouncements } from "../../a11y/announcer";
import "../../i18n";
import { axeViolations } from "../../test/axe";
import { resetServerStore } from "../channels/serverStore";
import { UserPanel } from "./UserPanel";

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

async function setup(connect = true, registered = false) {
  api = createMockApi({ seed: "ready" });
  setApi(api);
  resetServerStore();
  if (connect) {
    const [server] = await api.serversList();
    await api.connect((server ?? { id: 0 }).id, null);
  }
  render(
    <ViewContext.Provider value={view}>
      <UserPanel />
    </ViewContext.Provider>,
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
});

const mute = () => screen.getByRole("button", { name: "Mute microphone" });
const deafen = () => screen.getByRole("button", { name: "Deafen" });

async function openMenuItem(name: string) {
  const opener = document.querySelector('[aria-haspopup="menu"]');
  if (!opener) throw new Error("user menu button missing");
  fireEvent.click(opener);
  fireEvent.click(await screen.findByRole("menuitem", { name }));
}

describe("UserPanel", () => {
  it("toggles mute and reflects it", async () => {
    await setup();
    expect(mute()).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(mute());
    await waitFor(() => {
      expect(mute()).toHaveAttribute("aria-pressed", "true");
    });
    expect(announcements).toContain("Microphone muted");
  });

  it("deafen implies mute and undeafen restores it", async () => {
    await setup();
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
    expect(userButton().querySelector(".userpanel-avatar")?.textContent).not.toBe("");
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

  it("gives the name the available width and exposes it in full", async () => {
    await setup();
    const name = document.querySelector<HTMLElement>(".userpanel__name");
    if (!name) throw new Error("name missing");
    expect(name).toHaveAttribute("title", name.textContent);
    const button = document.querySelector<HTMLElement>('[aria-haspopup="menu"]');
    expect(button).toHaveAccessibleName(name.textContent);
    // jsdom does not apply the stylesheet, so assert the rules themselves.
    const css = readFileSync(
      resolve(process.cwd(), "ui/src/features/userpanel/userpanel.css"),
      "utf8",
    );
    const rule = (selector: string) =>
      new RegExp(`^${selector.replace(".", "\\.")} \\{([^}]*)\\}`, "m").exec(css)?.[1] ?? "";
    expect(rule(".userpanel__name")).toMatch(/flex: 1 1 auto;/);
    expect(rule(".userpanel__name")).toMatch(/min-width: 0;/);
    expect(rule(".userpanel__name")).toMatch(/text-overflow: ellipsis;/);
    expect(rule(".userpanel__toggle")).toMatch(/flex: none;/);
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
});
