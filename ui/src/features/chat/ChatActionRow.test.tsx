import {
  act,
  fireEvent,
  render as rtlRender,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi, WatchParty } from "../../api";
import { applyLanguage } from "../../i18n";
import { getServerState } from "../channels/serverStore";
import { selectConversation } from "../channels/selection";
import { connectedMock, resetAll } from "../channels/testUtils";
import { resetScreenShareStore } from "../screenshare";
import { resetWatchPartyStore } from "../watchparty";
import { ChatActionRow } from "./ChatActionRow";
import { renderWithEmbeds as render, withEmbeds } from "../../app/testPreferences";

const VIDEO = "dQw4w9WgXcQ";
const LINK = `https://www.youtube.com/watch?v=${VIDEO}`;

let api: MockApi;
let own: number;

beforeAll(async () => {
  await applyLanguage("en");
});

beforeEach(async () => {
  api = await connectedMock({ bindStore: true });
  api.mock.emit("voiceServerChanged", { serverId: 1 });
  const { users, selfSession } = getServerState();
  own = users.get(selfSession ?? -1)?.channelId ?? -1;
  selectConversation({ kind: "channel", channelId: own });
});

afterEach(() => {
  vi.restoreAllMocks();
  resetWatchPartyStore();
  resetScreenShareStore();
  resetAll();
});

const appsButton = () => screen.queryByRole("button", { name: /^Apps/ });
const openMenu = () => {
  fireEvent.click(screen.getByRole("button", { name: /^Apps/ }));
  return screen.getByRole("menu", { name: "Start together" });
};
const withOs = (os: string) => {
  const info = api.appInfo.bind(api);
  vi.spyOn(api, "appInfo").mockImplementation(async () => ({ ...(await info()), os }));
};
const asLinux = () => {
  withOs("linux x86_64");
};

describe("visibility", () => {
  it("shows the Apps button in the chat of the own voice channel", () => {
    render(<ChatActionRow />);
    expect(appsButton()).toBeInTheDocument();
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("is gone in another channel's chat", () => {
    const other = [...getServerState().channels.keys()].find((id) => id !== own) ?? -1;
    selectConversation({ kind: "channel", channelId: other });
    const { container } = render(<ChatActionRow />);
    expect(container).toBeEmptyDOMElement();
  });

  it("is gone in a direct message", () => {
    selectConversation({ kind: "private", certHash: "abc" });
    const { container } = render(<ChatActionRow />);
    expect(container).toBeEmptyDOMElement();
  });
});

describe("menu", () => {
  it("offers Share screen and Watch together", () => {
    render(<ChatActionRow />);
    openMenu();
    expect(screen.getByRole("menuitem", { name: /^Share screen/ })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Watch together" })).toBeInTheDocument();
  });

  it("shows the reason next to a disabled entry and does not act", async () => {
    withOs("plan9");
    const start = vi.spyOn(api, "screenShareStart");
    render(<ChatActionRow />);
    openMenu();
    const share = screen.getByRole("menuitem", { name: /^Share screen/ });
    await waitFor(() => {
      expect(share).toHaveAttribute("aria-disabled", "true");
    });
    expect(share).toHaveTextContent("isn't available on this system yet");
    fireEvent.click(share);
    expect(start).not.toHaveBeenCalled();
    expect(screen.getByRole("menu")).toBeInTheDocument();
  });

  it("moves with the arrow keys and closes with Escape, returning the focus", () => {
    render(<ChatActionRow />);
    const menu = openMenu();
    const [first, second] = screen.getAllByRole("menuitem");
    expect(first).toHaveFocus();
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(second).toHaveFocus();
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(first).toHaveFocus();
    fireEvent.keyDown(menu, { key: "ArrowUp" });
    expect(second).toHaveFocus();
    fireEvent.keyDown(menu, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(appsButton()).toHaveFocus();
  });

  it("closes on a click outside", () => {
    render(<ChatActionRow />);
    openMenu();
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("menu")).toBeNull();
  });
});

describe("share toggle", () => {
  it("shows the live dot and pause, stop and viewers while sharing", async () => {
    asLinux();
    render(<ChatActionRow />);
    expect(appsButton()).toHaveAccessibleName("Apps");
    openMenu();
    const share = screen.getByRole("menuitem", { name: /^Share screen/ });
    await waitFor(() => {
      expect(share).not.toHaveAttribute("aria-disabled");
    });
    fireEvent.click(share);
    await waitFor(() => {
      expect(appsButton()).toHaveAccessibleName("Apps (you are sharing your screen)");
    });
    expect(appsButton()?.querySelector(".chat-apps__live")).not.toBeNull();
    openMenu();
    expect(screen.queryByRole("menuitem", { name: /^Share screen/ })).toBeNull();
    expect(screen.getByRole("menuitem", { name: "Pause sharing" })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Who is watching" })).toHaveTextContent(
      "Nobody is watching yet",
    );
    fireEvent.click(screen.getByRole("menuitem", { name: "Stop sharing" }));
    await waitFor(() => {
      expect(appsButton()).toHaveAccessibleName("Apps");
    });
  });
});

describe("watch together", () => {
  const open = () => {
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Watch together" }));
    return screen.getByRole("textbox", { name: "YouTube link" });
  };

  it("starts the party without posting a chat message", async () => {
    const start = vi.spyOn(api, "watchPartyStart");
    const post = vi.spyOn(api, "sendChannelMessage");
    render(<ChatActionRow />);
    fireEvent.change(open(), { target: { value: ` ${LINK} ` } });
    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    await waitFor(() => {
      expect(start).toHaveBeenCalledWith(1, VIDEO);
    });
    expect(post).not.toHaveBeenCalled();
  });

  it("rejects a link that is not a YouTube video", () => {
    const start = vi.spyOn(api, "watchPartyStart");
    render(<ChatActionRow />);
    fireEvent.change(open(), { target: { value: "https://example.com/watch?v=" + VIDEO } });
    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    expect(screen.getByRole("alert")).toHaveTextContent("not a YouTube video link");
    expect(start).not.toHaveBeenCalled();
  });

  it("opens a dialog and closes it with Cancel and Escape", () => {
    render(<ChatActionRow />);
    open();
    expect(screen.getByRole("dialog", { name: "Watch together" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    open();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("uses the themed input class", () => {
    render(<ChatActionRow />);
    expect(open()).toHaveClass("settings-input");
  });

  it("offers the playlist instead of a second party while one runs", async () => {
    const party: WatchParty = {
      channelId: own,
      videoId: VIDEO,
      playing: true,
      positionMs: 0,
      participants: [],
      lastAction: null,
      joined: true,
      currentEntry: 0,
      playlist: [],
      skippedBy: null,

      positions: [],

      host: 0,
      history: [],
    };
    const add = vi.spyOn(api, "watchPartyAdd").mockResolvedValue(1);
    const start = vi.spyOn(api, "watchPartyStart");
    const post = vi.spyOn(api, "sendChannelMessage");
    render(<ChatActionRow />);
    act(() => {
      api.mock.emit("watchPartyChanged", { serverId: 1, party });
    });
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Add to playlist" }));
    expect(screen.getByRole("region", { name: "Up next" })).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "YouTube link" }), {
      target: { value: LINK },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    await waitFor(() => {
      expect(add).toHaveBeenCalledWith(1, VIDEO);
    });
    expect(post).not.toHaveBeenCalled();
    expect(start).not.toHaveBeenCalled();
  });
});

describe("embedded players switched off", () => {
  const openPrivacySettings = vi.fn();
  const renderOff = () => {
    rtlRender(<ChatActionRow />, { wrapper: withEmbeds(false, { openPrivacySettings }) });
  };

  it("lists Watch together disabled with the reason and a link to the privacy settings", () => {
    renderOff();
    const menu = openMenu();
    const watch = within(menu).getByRole("menuitem", { name: /^Watch together/ });
    expect(watch).toHaveAttribute("aria-disabled", "true");
    expect(watch).toHaveTextContent(
      "Turn on embedded players in Privacy settings to watch together",
    );
    fireEvent.click(watch);
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Open Privacy settings" }));
    expect(openPrivacySettings).toHaveBeenCalledTimes(1);
  });

  it("starts Watch together as before when the switch is on", () => {
    render(<ChatActionRow />);
    const menu = openMenu();
    const watch = within(menu).getByRole("menuitem", { name: /^Watch together/ });
    expect(watch).not.toHaveAttribute("aria-disabled");
    expect(within(menu).queryByRole("menuitem", { name: "Open Privacy settings" })).toBeNull();
    expect(watch).not.toHaveTextContent("Privacy settings");
  });
});
