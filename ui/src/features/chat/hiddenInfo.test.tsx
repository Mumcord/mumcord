import { act, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { getServerState, resetServerStore, type ServerState } from "../channels/serverStore";
import { pinnedInfo } from "./pinnedInfoModel";
import {
  descriptionKey,
  hideInfo,
  resetHiddenInfo,
  showInfo,
  textHash,
  useHiddenHash,
  welcomeKey,
} from "./hiddenInfo";

function serverWith(welcomeText: string): ServerState {
  resetServerStore();
  const base = getServerState();
  return {
    ...base,
    info: {
      version: "1.5",
      followSupported: true,
      welcomeText,
      defaultChannelId: 0,
      maxMessageLength: 0,
      maxImageMessageLength: 0,
      allowHtml: true,
    },
    channels: new Map([
      [
        0,
        {
          id: 0,
          parentId: null,
          name: "Root",
          position: 0,
          permissions: { enter: true, textMessage: true, listen: true, registerSelf: false },
          description: null,
          hasDescription: false,
        },
      ],
    ]),
  };
}
const root = { kind: "channel", channelId: 0 } as const;

function Probe({ id }: { id: number }) {
  return <p>{useHiddenHash(welcomeKey(id)) ?? "visible"}</p>;
}

beforeEach(() => {
  localStorage.clear();
  resetHiddenInfo();
});

describe("welcome message hiding", () => {
  it("hides, persists across a reload, and is per server", () => {
    hideInfo(welcomeKey(1), "Hello");
    expect(localStorage.getItem("mumcord.hidden.welcome.1")).toBe(textHash("Hello"));
    resetHiddenInfo(); // simulates a restart: state comes from storage
    const state = serverWith("Hello");
    expect(
      pinnedInfo(state, root, {
        welcome: localStorage.getItem("mumcord.hidden.welcome.1"),
        description: null,
      }).blocks,
    ).toEqual([]);
    expect(pinnedInfo(state, root).blocks).toEqual(["Hello"]);
  });

  it("shows again when the welcome text changes", () => {
    hideInfo(welcomeKey(1), "Hello");
    const hash = textHash("Hello");
    expect(
      pinnedInfo(serverWith("Hello"), root, { welcome: hash, description: null }).welcome,
    ).toBeNull();
    expect(
      pinnedInfo(serverWith("Hello v2"), root, { welcome: hash, description: null }).welcome,
    ).toBe("Hello v2");
  });

  it("restores with showWelcome and notifies subscribers", () => {
    render(<Probe id={2} />);
    expect(screen.getByText("visible")).toBeInTheDocument();
    act(() => {
      hideInfo(welcomeKey(2), "x");
    });
    expect(screen.queryByText("visible")).toBeNull();
    act(() => {
      showInfo(welcomeKey(2));
    });
    expect(screen.getByText("visible")).toBeInTheDocument();
    expect(localStorage.getItem("mumcord.hidden.welcome.2")).toBeNull();
  });

  it("hides a channel description per channel and shows it again when it changes", () => {
    const withDescription = (text: string) => {
      const state = serverWith("Welcome");
      const root = state.channels.get(0);
      if (!root) throw new Error("no root");
      const channels = new Map(state.channels);
      channels.set(0, { ...root, description: text, hasDescription: true });
      return { ...state, channels };
    };
    hideInfo(descriptionKey(1, 0), "Rules");
    const hidden = localStorage.getItem("mumcord.hidden.description.1.0");
    const shown = pinnedInfo(withDescription("Rules"), root, {
      welcome: null,
      description: hidden,
    });
    expect(shown.description).toBeNull();
    expect(shown.welcome).toBe("Welcome");
    const changed = pinnedInfo(withDescription("New rules"), root, {
      welcome: null,
      description: hidden,
    });
    expect(changed.description).toBe("New rules");
    expect(descriptionKey(1, 0)).not.toBe(descriptionKey(1, 5));
  });
});
