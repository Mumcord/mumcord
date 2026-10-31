import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { ChannelSearch } from "../channels/ChannelSearch";
import { connectedMock, resetAll } from "../channels/testUtils";
import type { SendTarget } from "./conversationInfo";
import { Composer } from "./Composer";

const LIMITS = { text: 5000, image: 100_000, allowHtml: true };

function renderComposer(target: SendTarget = { kind: "channel", channelId: 1 }) {
  const { container } = render(
    <main>
      <ChannelSearch value="" onChange={() => {}} onClear={() => {}} onSubmit={() => {}} />
      <Composer target={target} limits={LIMITS} label="Message #Lobby" blockedReason={null} />
    </main>,
  );
  const input = screen.getByRole<HTMLTextAreaElement>("textbox", { name: "Message #Lobby" });
  return { container, input };
}

function type(input: HTMLTextAreaElement, text: string, start: number, end = start) {
  fireEvent.change(input, { target: { value: text } });
  input.focus();
  input.setSelectionRange(start, end);
}

beforeAll(async () => {
  await applyLanguage("en");
});

describe("Composer link preview", () => {
  it("shows the card of a `mumble://` link before it is sent, without its credentials", async () => {
    const { input } = renderComposer();
    type(input, "join mumble://Maya:secret@voice.example.org:64739/Raid", 0);
    const card = await screen.findByRole("article", { name: "Server card for voice.example.org" });
    expect(card).toHaveTextContent("Channel Raid");
    expect(card.textContent).not.toContain("secret");
    // Nothing is joined from the preview: the card in the composer has no buttons.
    expect(within(card).queryByRole("button")).toBeNull();
  });

  it("shows no card for a link that is not a server link", async () => {
    const { input } = renderComposer();
    type(input, "mumble:///Channel", 0);
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(screen.queryByRole("article", { name: /Server card/ })).toBeNull();
  });
});

describe("Composer formatting", () => {
  it("wraps the selection in bold with Ctrl+B and keeps it selected", () => {
    const { input } = renderComposer();
    type(input, "say hi", 4, 6);
    fireEvent.keyDown(input, { key: "b", ctrlKey: true });
    expect(input.value).toBe("say **hi**");
    expect([input.selectionStart, input.selectionEnd]).toEqual([6, 8]);
  });

  it("inserts italic markers with the caret between them from the toolbar", () => {
    const { input } = renderComposer();
    type(input, "a ", 2);
    const button = screen.getByRole("button", { name: "Italic" });
    // The toolbar must not take the focus away from the input.
    expect(fireEvent.mouseDown(button)).toBe(false);
    fireEvent.click(button);
    expect(input.value).toBe("a **");
    expect(document.activeElement).toBe(input);
    expect([input.selectionStart, input.selectionEnd]).toEqual([3, 3]);
  });

  it("inserts a code block with Ctrl+Shift+E", () => {
    const { input } = renderComposer();
    type(input, "", 0);
    fireEvent.keyDown(input, { key: "E", ctrlKey: true, shiftKey: true });
    expect(input.value).toBe("```\n\n```");
    expect(input.selectionStart).toBe(4);
  });

  it("uses Ctrl+K for a link inside the input, for the channel search elsewhere", () => {
    const { input } = renderComposer();
    const search = screen.getByRole("searchbox");
    type(input, "docs", 0, 4);
    fireEvent.keyDown(input, { key: "k", ctrlKey: true });
    expect(input.value).toBe("[docs]()");
    expect(document.activeElement).toBe(input);

    input.blur();
    fireEvent.keyDown(document.body, { key: "k", ctrlKey: true });
    expect(document.activeElement).toBe(search);
  });

  it("offers a labelled toolbar with shortcuts in the tooltips and one Tab stop", async () => {
    const { container } = renderComposer();
    const toolbar = screen.getByRole("toolbar", { name: "Formatting" });
    const buttons = Array.from(toolbar.querySelectorAll("button"));
    expect(buttons.map((b) => b.getAttribute("aria-label"))).toEqual([
      "Bold",
      "Italic",
      "Strikethrough",
      "Inline code",
      "Code block",
      "Link",
      "Quote",
    ]);
    expect(buttons[0]?.dataset.tooltip).toBe("Bold (Ctrl+B)");
    expect(buttons.filter((b) => b.tabIndex === 0)).toHaveLength(1);
    buttons[0]?.focus();
    fireEvent.keyDown(buttons[0] as HTMLButtonElement, { key: "ArrowLeft" });
    expect(document.activeElement).toBe(buttons[6]);
    expect(await axeViolations(container)).toEqual([]);
  });
});

describe("Composer mentions", () => {
  afterEach(() => {
    resetAll();
  });

  async function setup(target?: SendTarget) {
    await connectedMock();
    const view = renderComposer(target);
    await waitFor(() => {
      expect(view.input).toBeEnabled();
    });
    // The seeded server needs a moment to sync its people.
    await act(() => Promise.resolve());
    return view;
  }

  function names() {
    return within(screen.getByRole("listbox"))
      .getAllByRole("option")
      .map((o) => o.querySelector(".mention-picker__name, .mention-picker__keyword")?.textContent);
  }

  it("lists people on typing @, people of the current channel first", async () => {
    const { input } = await setup();
    type(input, "@", 1);
    expect(await screen.findByRole("listbox")).toBeInTheDocument();
    expect(input).toHaveAttribute("role", "combobox");
    expect(input).toHaveAttribute("aria-expanded", "true");
    expect(names()).toEqual(["@channel", "@all", "Anna", "Ben", "Carla"]);
    type(input, "@c", 2);
    expect(names()).toEqual(["@channel", "Carla"]);
  });

  it("offers @channel and @all in a channel with what they reach, and inserts the keyword", async () => {
    const { input } = await setup();
    type(input, "Heads up @", 10);
    await screen.findByRole("listbox");
    expect(screen.getByRole("group", { name: "Mentions" })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "People" })).toBeInTheDocument();
    expect(screen.getAllByRole("option")[0]).toHaveTextContent(/^@channelNotify everyone in /u);
    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(input.value).toBe("Heads up @all ");
  });

  it("offers no @channel or @all in a private chat", async () => {
    const { input } = await setup({ kind: "private", session: 2 });
    type(input, "@", 1);
    await screen.findByRole("listbox");
    expect(names()).toEqual(["Anna", "Ben", "Carla"]);
    expect(screen.queryByRole("group")).toBeNull();
  });

  it("selects with arrow keys and Enter, inserting plain @Name without sending", async () => {
    const { input } = await setup();
    type(input, "hi @", 4);
    await screen.findByRole("listbox");
    for (let i = 0; i < 3; i += 1) fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(screen.getAllByRole("option")[3]).toHaveAttribute("aria-selected", "true");
    expect(input.getAttribute("aria-activedescendant")).toBe(screen.getAllByRole("option")[3]?.id);
    fireEvent.keyDown(input, { key: "Enter" });
    expect(input.value).toBe("hi @Ben ");
    expect([input.selectionStart, input.selectionEnd]).toEqual([8, 8]);
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("selects with Tab, closes with Escape and picks with the mouse", async () => {
    const { input } = await setup();
    type(input, "@an", 3);
    await screen.findByRole("listbox");
    fireEvent.keyDown(input, { key: "Tab" });
    expect(input.value).toBe("@Anna ");
    type(input, "@", 1);
    await screen.findByRole("listbox");
    fireEvent.keyDown(input, { key: "Escape" });
    expect(screen.queryByRole("listbox")).toBeNull();
    type(input, "x @", 3);
    fireEvent.click(await screen.findByRole("option", { name: "Carla" }));
    expect(input.value).toBe("x @Carla ");
  });

  it("keeps the formatting shortcuts working while the list is open", async () => {
    const { input } = await setup();
    type(input, "@a", 2);
    await screen.findByRole("listbox");
    fireEvent.keyDown(input, { key: "b", ctrlKey: true });
    expect(input.value).toBe("@a****");
  });
});

describe("Composer replies", () => {
  afterEach(() => {
    resetAll();
  });

  it("shows the reply preview, sends the quote first and cancels with Escape", async () => {
    const api = await connectedMock();
    const sent: string[] = [];
    const original = api.sendChannelMessage.bind(api);
    vi.spyOn(api, "sendChannelMessage").mockImplementation((serverId, channel, html) => {
      sent.push(html);
      return original(serverId, channel, html);
    });
    const onCancel = vi.fn();
    const { rerender } = render(
      <Composer
        target={{ kind: "channel", channelId: 1 }}
        limits={LIMITS}
        label="Message #Lobby"
        blockedReason={null}
        reply={{ name: "Maya", snippet: "<b>hi</b>" }}
        onCancelReply={onCancel}
      />,
    );
    const input = screen.getByRole<HTMLTextAreaElement>("textbox", { name: "Message #Lobby" });
    await waitFor(() => {
      expect(input).toHaveFocus();
    });
    expect(screen.getByText(/Replying to/)).toHaveTextContent("Replying to Maya: <b>hi</b>");

    fireEvent.keyDown(input, { key: "Escape" });
    expect(onCancel).toHaveBeenCalledTimes(1);

    fireEvent.change(input, { target: { value: "yes" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => {
      expect(sent).toEqual(["<blockquote><b>Maya</b>: &lt;b&gt;hi&lt;/b&gt;</blockquote>yes"]);
    });
    expect(onCancel).toHaveBeenCalledTimes(2);

    rerender(
      <Composer
        target={{ kind: "channel", channelId: 1 }}
        limits={LIMITS}
        label="Message #Lobby"
        blockedReason={null}
        reply={{ name: "Maya", snippet: "hi" }}
        onCancelReply={onCancel}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Cancel reply" }));
    expect(onCancel).toHaveBeenCalledTimes(3);
  });
});

describe("Composer cards", () => {
  afterEach(() => {
    resetAll();
  });

  it("sends typed card BB code as card HTML that official clients can read", async () => {
    const api = await connectedMock();
    const sent: string[] = [];
    const original = api.sendChannelMessage.bind(api);
    vi.spyOn(api, "sendChannelMessage").mockImplementation((serverId, channel, html) => {
      sent.push(html);
      return original(serverId, channel, html);
    });
    render(
      <Composer
        target={{ kind: "channel", channelId: 1 }}
        limits={LIMITS}
        label="Message #Lobby"
        blockedReason={null}
      />,
    );
    const input = screen.getByRole<HTMLTextAreaElement>("textbox", { name: "Message #Lobby" });
    fireEvent.change(input, {
      target: {
        value: "[card title=Deploy color=info url=https://ci.test/1]Version **2.8**[/card]",
      },
    });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => {
      expect(sent).toEqual([
        '<div data-mumcord-card="v=1&amp;title=Deploy&amp;color=info&amp;url=https%3A%2F%2Fci.test%2F1">' +
          '<b>[Info] Deploy</b><div data-mumcord-card="body">Version <strong>2.8</strong></div>' +
          '<a href="https://ci.test/1">https://ci.test/1</a></div>',
      ]);
    });
  });
});

describe("Composer code blocks", () => {
  afterEach(() => {
    resetAll();
  });

  it("sends the fence language as a marker on the <pre> (issue #112)", async () => {
    const api = await connectedMock();
    const sent: string[] = [];
    const original = api.sendChannelMessage.bind(api);
    vi.spyOn(api, "sendChannelMessage").mockImplementation((serverId, channel, html) => {
      sent.push(html);
      return original(serverId, channel, html);
    });
    render(
      <Composer
        target={{ kind: "channel", channelId: 1 }}
        limits={LIMITS}
        label="Message #Lobby"
        blockedReason={null}
      />,
    );
    const input = screen.getByRole<HTMLTextAreaElement>("textbox", { name: "Message #Lobby" });
    fireEvent.change(input, { target: { value: "```sql\nSELECT * FROM users;\n```" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => {
      expect(sent).toEqual([
        '<pre data-mumcord-lang="sql"><code>SELECT * FROM users;\n</code></pre>',
      ]);
    });
  });
});

describe("Composer Apps button", () => {
  afterEach(resetAll);

  it("sits directly before the image picker and has no menu until opened", async () => {
    const api = await connectedMock({ bindStore: true });
    api.mock.emit("voiceServerChanged", { serverId: 1 });
    const { selectConversation } = await import("../channels/selection");
    const { getServerState } = await import("../channels/serverStore");
    const { users, selfSession } = getServerState();
    selectConversation({
      kind: "channel",
      channelId: users.get(selfSession ?? -1)?.channelId ?? -1,
    });
    renderComposer();
    const apps = screen.getByRole("button", { name: "Apps" });
    expect(apps.parentElement?.nextElementSibling).toBe(
      screen.getByRole("button", { name: "Add image" }),
    );
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("is not shown when the chat is not the voice channel's", () => {
    renderComposer();
    expect(screen.queryByRole("button", { name: "Apps" })).toBeNull();
  });
});
