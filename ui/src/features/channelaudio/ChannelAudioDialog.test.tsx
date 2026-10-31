import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { MockApi } from "../../api";
import { axeViolations } from "../../test/axe";
import { selectConversation } from "../channels";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { ChatView } from "../chat";
import { resetChatStore } from "../chat/chatStore";
import { ChannelHeader } from "../channelheader";
import { resetFollowStore } from "../follow";
import { ChannelAudioHost } from "./ChannelAudioHost";
import { openChannelAudioDialog } from "./opener";

let api: MockApi;

async function setup(channelId = 1) {
  resetFollowStore();
  api = await connectedMock({ bindStore: true });
  resetChatStore();
  stubDialog();
  render(
    <>
      <ChatView renderChannelTitle={(id) => <ChannelHeader channelId={id} />} />
      <ChannelAudioHost />
    </>,
  );
  act(() => {
    selectConversation({ kind: "channel", channelId });
  });
  const heading = await screen.findByRole("heading", { level: 2 });
  return within(heading).getByRole("button");
}

function openAudio(name: HTMLElement) {
  fireEvent.click(name);
  fireEvent.click(
    within(screen.getByRole("menu")).getByRole("menuitem", { name: /Audio for this channel/ }),
  );
  return screen.getByRole("dialog", { name: "Audio for this channel" });
}

afterEach(() => {
  resetAll();
});

describe("Audio for this channel", () => {
  it("saves a preset, shows the badge and the menu hint, and removes it again", async () => {
    const name = await setup();
    const dialog = openAudio(name);
    expect(within(dialog).getByRole("radio", { name: /Voice/ })).toBeChecked();
    expect(within(dialog).queryByRole("button", { name: "Remove profile" })).toBeNull();
    fireEvent.click(within(dialog).getByRole("radio", { name: /Music/ }));
    expect(within(dialog).getByRole("switch", { name: /Music quality/ })).toBeChecked();
    expect(within(dialog).getByRole("slider", { name: "Bitrate" })).toHaveValue("256");
    expect(await axeViolations(dialog)).toEqual([]);
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

    expect(await screen.findByText("Music profile")).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(await api.channelAudioProfiles(1)).toEqual([
      { channelId: 1, profile: { musicQuality: true, lowLatency: null, bitrateKbps: 256 } },
    ]);
    fireEvent.click(name);
    expect(
      within(screen.getByRole("menu")).getByRole("menuitem", { name: /Audio for this channel/ }),
    ).toHaveTextContent("♪ Music");
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });

    const again = openAudio(name);
    expect(within(again).getByRole("radio", { name: /Music/ })).toBeChecked();
    fireEvent.click(within(again).getByRole("button", { name: "Remove profile" }));
    await waitFor(() => {
      expect(screen.queryByText("Music profile")).toBeNull();
    });
    expect(await api.channelAudioProfiles(1)).toEqual([]);
  });

  it("keeps a value on the global setting until it is changed, and back with use default", async () => {
    const name = await setup();
    const dialog = openAudio(name);
    expect(await within(dialog).findAllByText("Default (off)")).toHaveLength(2);
    fireEvent.click(within(dialog).getByRole("switch", { name: /Low-latency mode/ }));
    // Low-latency mode includes music quality.
    expect(within(dialog).getByRole("switch", { name: /Music quality/ })).toBeDisabled();
    expect(within(dialog).getByRole("radio", { name: /Jam/ })).toBeChecked();
    fireEvent.change(within(dialog).getByRole("slider", { name: "Bitrate" }), {
      target: { value: "128" },
    });
    expect(within(dialog).queryByRole("radio", { checked: true })).toBeNull();
    for (const button of within(dialog).getAllByRole("button", { name: "use default" })) {
      fireEvent.click(button);
    }
    expect(within(dialog).getByRole("radio", { name: /Voice/ })).toBeChecked();
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(await api.channelAudioProfiles(1)).toEqual([]);
  });

  it("applies while the user is in the channel and leaves the global settings alone", async () => {
    await setup();
    const self = (await api.sessionSnapshot(1))?.users.find((u) => u.session === 1);
    const here = self?.channelId ?? 0;
    const other = here === 1 ? 2 : 1;
    await api.channelAudioProfileSet(1, other, {
      musicQuality: null,
      lowLatency: true,
      bitrateKbps: null,
    });
    expect(await api.activeAudioProfile()).toBeNull();
    await api.channelJoin(1, other);
    expect(await api.activeAudioProfile()).toMatchObject({ serverId: 1, channelId: other });
    expect((await api.settingsGet()).lowLatency).toBe(false);
    await api.channelJoin(1, here);
    expect(await api.activeAudioProfile()).toBeNull();
  });

  it("opens for any channel id through the opener, and from channel info", async () => {
    const name = await setup();
    act(() => {
      openChannelAudioDialog(1);
    });
    const dialog = screen.getByRole("dialog", { name: "Audio for this channel" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    fireEvent.click(name);
    fireEvent.click(
      within(screen.getByRole("menu")).getByRole("menuitem", { name: "Channel info" }),
    );
    const info = screen.getByRole("dialog", { name: /Channel info/ });
    expect(within(info).getByText("My audio profile")).toBeInTheDocument();
    fireEvent.click(within(info).getByRole("button", { name: "Set up…" }));
    expect(screen.getByRole("dialog", { name: "Audio for this channel" })).toBeInTheDocument();
  });
});
