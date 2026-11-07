import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { DragDropEvent } from "@tauri-apps/api/webview";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi } from "../../api";
import type { FileChip, RelayInfo } from "../../bindings/commands";
import { ViewContext, type View } from "../../app/view";
import { chipStatus } from "../files/composer/chipView";
import { connectedMock, resetAll } from "../channels/testUtils";
import { i18n } from "../../i18n";
import { conversationMessages } from "./chatStore";
import { Composer } from "./Composer";
import { closeRelayDetails, getRelayDetails } from "../p2p/relayTrust";

const dropHandlers = vi.hoisted(() => new Set<(event: { payload: DragDropEvent }) => void>());
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: (handler: (event: { payload: DragDropEvent }) => void) => {
      dropHandlers.add(handler);
      return Promise.resolve(() => dropHandlers.delete(handler));
    },
  }),
}));

function emit(payload: DragDropEvent) {
  act(() => {
    for (const handler of dropHandlers) handler({ payload });
  });
}

function at(x: number, y: number) {
  return { x, y } as Extract<DragDropEvent, { type: "over" }>["position"];
}

function chip(id: number, overrides: Partial<FileChip> = {}): FileChip {
  return {
    id,
    name: `file${String(id)}.pdf`,
    size: 18_400_000,
    state: { kind: "ready" },
    ...overrides,
  };
}

let api: MockApi;

beforeEach(async () => {
  api = await connectedMock();
  Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
  Object.defineProperty(window, "devicePixelRatio", { value: 1, configurable: true });
  vi.spyOn(HTMLDivElement.prototype, "getBoundingClientRect").mockReturnValue(
    new DOMRect(0, 0, 1000, 800),
  );
  vi.spyOn(HTMLFormElement.prototype, "getBoundingClientRect").mockReturnValue(
    new DOMRect(0, 700, 1000, 100),
  );
});

afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  Reflect.deleteProperty(window, "devicePixelRatio");
  vi.restoreAllMocks();
  resetAll();
});

function setup(
  target: { kind: "channel"; channelId: number } | { kind: "private"; session: number },
) {
  return render(
    <div className="chat">
      <Composer
        target={target}
        limits={{ text: 5000, image: 100_000, allowHtml: true }}
        label="Message #General"
        blockedReason={null}
      />
    </div>,
  );
}

const CHANNEL = { kind: "channel", channelId: 1 } as const;

/** A relay announcement change; the composer re-reads the availability on any of them. */
const RELAY_NONE: RelayInfo = {
  bots: [],
  host: null,
  status: { kind: "none" },
  firstSeen: null,
  uses: [],
  broadcastMax: null,
  fingerprint: null,
  previousFingerprint: null,
};

describe("chipStatus", () => {
  const t = i18n.getFixedT("en", "files");
  it("shows size, progress and errors", () => {
    expect(chipStatus(t, "en", chip(1))).toBe("18.4 MB · peer-to-peer");
    expect(chipStatus(t, "en", chip(1, { state: { kind: "zipping", done: 64, total: 100 } }))).toBe(
      "Zipping folder… 64 %",
    );
    expect(chipStatus(t, "en", chip(1, { state: { kind: "checking", done: 1, total: 4 } }))).toBe(
      "Checking… 25 %",
    );
    expect(
      chipStatus(
        t,
        "en",
        chip(1, { state: { kind: "failed", error: { kind: "tooLarge", name: "a.iso" } } }),
      ),
    ).toBe("a.iso is larger than 2 GB and can't be shared");
  });
});

describe("Composer file sharing", () => {
  it("disables the paperclip with the reason when the server has no relay", async () => {
    vi.spyOn(api, "filesAvailability").mockResolvedValue({ kind: "noRelay" });
    setup(CHANNEL);
    const reason = "This server has no relay, so file sharing isn't available.";
    expect(await screen.findByText(reason)).toBeTruthy();
    const button = screen.getByRole("button", { name: "Share files in this channel" });
    expect(button.getAttribute("aria-disabled")).toBe("true");
    expect(button.getAttribute("data-tooltip")).toBe(reason);
  });

  it("closes the no-relay hint for this server and keeps the reason on the paperclip (#579)", async () => {
    localStorage.clear();
    const availability = vi.spyOn(api, "filesAvailability").mockResolvedValue({ kind: "noRelay" });
    const reason = "This server has no relay, so file sharing isn't available.";
    const { unmount } = setup(CHANNEL);
    expect(await screen.findByText(reason)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /^Hide this hint for/ }));
    expect(screen.queryByText(reason)).toBeNull();
    const button = screen.getByRole("button", { name: "Share files in this channel" });
    expect(button.getAttribute("data-tooltip")).toBe(reason);
    unmount();

    // Still hidden after a restart.
    const before = availability.mock.calls.length;
    setup(CHANNEL);
    await waitFor(() => {
      expect(availability.mock.calls.length).toBeGreaterThan(before);
    });
    expect(screen.getByRole("button", { name: "Share files in this channel" })).toHaveAttribute(
      "data-tooltip",
      reason,
    );
    expect(screen.queryByText(reason)).toBeNull();
    localStorage.clear();
  });

  it("shows the closed no-relay hint again after a relay came and went (#579)", async () => {
    localStorage.clear();
    let relay = false;
    vi.spyOn(api, "filesAvailability").mockImplementation(() =>
      Promise.resolve(relay ? null : { kind: "noRelay" }),
    );
    const reason = "This server has no relay, so file sharing isn't available.";
    const paperclip = () => screen.getByRole("button", { name: "Share files in this channel" });
    setup(CHANNEL);
    expect(await screen.findByText(reason)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /^Hide this hint for/ }));
    expect(screen.queryByText(reason)).toBeNull();

    relay = true;
    act(() => {
      api.mock.emit("relayChanged", { serverId: 1, info: RELAY_NONE });
    });
    await waitFor(() => {
      expect(paperclip()).not.toHaveAttribute("aria-disabled", "true");
    });

    relay = false;
    act(() => {
      api.mock.emit("relayChanged", { serverId: 1, info: RELAY_NONE });
    });
    expect(await screen.findByText(reason)).toBeTruthy();
    localStorage.clear();
  });

  it("names a changed relay and links to its details instead of saying there is none (#600)", async () => {
    vi.spyOn(api, "filesAvailability").mockResolvedValue({ kind: "noRelay" });
    vi.spyOn(api, "relayInfo").mockResolvedValue({
      ...RELAY_NONE,
      status: { kind: "untrusted", keptBlocked: false },
    });
    setup(CHANNEL);
    expect(await screen.findByText(/the relay of this server changed/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: /^Hide this hint for/ })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Relay changed – review" }));
    expect(getRelayDetails()).toBe(1);
    closeRelayDetails();
  });

  it("reads the availability again when the relay changes or the settings close", async () => {
    const availability = vi
      .spyOn(api, "filesAvailability")
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce({ kind: "noRelay" })
      .mockResolvedValue({ kind: "p2pMode", mode: "directOnly" });
    const view = (overlay: "settings" | null) => ({ overlay }) as unknown as View;
    const { rerender } = render(
      <ViewContext.Provider value={view(null)}>
        <div className="chat">
          <Composer
            target={CHANNEL}
            limits={{ text: 5000, image: 100_000, allowHtml: true }}
            label="Message #General"
            blockedReason={null}
          />
        </div>
      </ViewContext.Provider>,
    );
    await waitFor(() => {
      expect(availability).toHaveBeenCalledTimes(1);
    });
    act(() => {
      api.mock.emit("relayChanged", { serverId: 1, info: RELAY_NONE });
    });
    expect(
      await screen.findByText("This server has no relay, so file sharing isn't available."),
    ).toBeTruthy();
    const again = (overlay: "settings" | null) => (
      <ViewContext.Provider value={view(overlay)}>
        <div className="chat">
          <Composer
            target={CHANNEL}
            limits={{ text: 5000, image: 100_000, allowHtml: true }}
            label="Message #General"
            blockedReason={null}
          />
        </div>
      </ViewContext.Provider>
    );
    rerender(again("settings"));
    rerender(again(null));
    expect(
      await screen.findByText(
        "File sharing needs the relay; your peer-to-peer setting is Direct only.",
      ),
    ).toBeTruthy();
  });

  it("has no paperclip in a private chat and refuses a drop on the chat", () => {
    setup({ kind: "private", session: 2 });
    expect(screen.queryByRole("button", { name: "Share files in this channel" })).toBeNull();
    emit({ type: "drop", paths: ["/a.pdf"], position: at(100, 100) });
    expect(screen.getByRole("alert").textContent).toBe(
      "Files can only be shared in channels for now",
    );
  });

  it("shows the overlay while dragging, then turns a drop into a chip with remove", async () => {
    const add = vi.spyOn(api, "filesChipAdd").mockResolvedValue(chip(1));
    const remove = vi.spyOn(api, "filesChipRemove");
    setup(CHANNEL);
    emit({ type: "over", position: at(100, 100) });
    expect(screen.getByText(/Drop to share with/)).toBeTruthy();
    emit({ type: "drop", paths: ["/tmp/file1.pdf"], position: at(100, 100) });
    expect(await screen.findByText("file1.pdf")).toBeTruthy();
    expect(add).toHaveBeenCalledWith("/tmp/file1.pdf");
    expect(screen.queryByText(/Drop to share with/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Remove file1.pdf" }));
    expect(remove).toHaveBeenCalledWith(1);
    expect(screen.queryByText("file1.pdf")).toBeNull();
  });

  it("keeps every file of a drop while the first one's events re-render the tray", async () => {
    let second: (value: FileChip) => void = () => undefined;
    vi.spyOn(api, "filesChipAdd")
      .mockResolvedValueOnce(chip(1, { state: { kind: "checking", done: 0, total: 100 } }))
      .mockImplementationOnce(
        () =>
          new Promise<FileChip>((resolve) => {
            second = resolve;
          }),
      );
    setup(CHANNEL);
    emit({ type: "drop", paths: ["/tmp/a.bin", "/tmp/b.bin"], position: at(100, 100) });
    expect(await screen.findByText("file1.pdf")).toBeTruthy();
    act(() => {
      emit({ type: "over", position: at(120, 120) });
      api.mock.emit("fileChipChanged", { chip: chip(1) });
      second(chip(2));
    });
    expect(await screen.findByText("file2.pdf")).toBeTruthy();
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
  });

  it("refuses an eleventh file", async () => {
    let next = 0;
    vi.spyOn(api, "filesChipAdd").mockImplementation(() => Promise.resolve(chip(++next)));
    setup(CHANNEL);
    const paths = Array.from({ length: 11 }, (_, i) => `/tmp/f${String(i)}`);
    emit({ type: "drop", paths, position: at(100, 100) });
    expect(await screen.findByText("You can share up to 10 files at once")).toBeTruthy();
    expect(screen.getAllByRole("listitem")).toHaveLength(10);
  });

  it("follows progress events and sends the ready files with the message", async () => {
    vi.spyOn(api, "filesChipAdd").mockResolvedValue(
      chip(1, { state: { kind: "zipping", done: 0, total: 100 } }),
    );
    const send = vi.spyOn(api, "filesSend").mockResolvedValue([]);
    setup(CHANNEL);
    emit({ type: "drop", paths: ["/tmp/dir"], position: at(100, 100) });
    expect(await screen.findByText("Zipping folder… 0 %")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Stop zipping and remove file1.pdf" }), {
      detail: 0,
    });
    // Removing the chip stops zipping; add it again to test the send path.
    emit({ type: "drop", paths: ["/tmp/dir"], position: at(100, 100) });
    await screen.findByText("Zipping folder… 0 %");
    // Nothing can go out while the file is still zipped (issue #596).
    expect(screen.getByRole("button", { name: "Send" })).toHaveAttribute("aria-disabled", "true");
    act(() => {
      api.mock.emit("fileChipChanged", { chip: chip(1, { state: { kind: "ready" } }) });
    });
    expect(await screen.findByText("18.4 MB · peer-to-peer")).toBeTruthy();
    // A ready file alone is enough to send, and the text field only asks for a comment.
    expect(screen.getByRole("textbox", { name: "Message #General" })).toHaveAttribute(
      "placeholder",
      "Add a message (optional)",
    );
    expect(screen.getByRole("button", { name: "Send" }).getAttribute("aria-disabled")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => {
      expect(send).toHaveBeenCalledWith(expect.any(Number), 1, [1]);
    });
  });

  it("shows the own offer messages in the chat after sending", async () => {
    vi.spyOn(api, "filesChipAdd").mockResolvedValue(chip(1));
    const offer = await api.sendChannelMessage(1, 1, "Alice shared a file");
    vi.spyOn(api, "filesSend").mockResolvedValue([offer]);
    setup(CHANNEL);
    emit({ type: "drop", paths: ["/tmp/a.pdf"], position: at(100, 100) });
    await screen.findByText("18.4 MB · peer-to-peer");
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => {
      expect(conversationMessages(CHANNEL, 1).map((m) => m.id)).toContain(offer.id);
    });
  });

  describe("dropping images", () => {
    it("shares non-image files at once without asking", async () => {
      const add = vi.spyOn(api, "filesChipAdd").mockResolvedValue(chip(1));
      setup(CHANNEL);
      emit({ type: "drop", paths: ["/tmp/a.pdf"], position: at(100, 100) });
      await waitFor(() => {
        expect(add).toHaveBeenCalledWith("/tmp/a.pdf");
      });
      expect(screen.queryByRole("dialog")).toBeNull();
    });

    it("asks for images and shares them as files on request", async () => {
      const add = vi
        .spyOn(api, "filesChipAdd")
        .mockImplementation((p) => Promise.resolve(chip(p.length)));
      const read = vi.spyOn(api, "readDroppedImage");
      setup(CHANNEL);
      emit({ type: "drop", paths: ["/tmp/cat.png"], position: at(100, 750) });
      const dialog = await screen.findByRole("dialog");
      expect(dialog.textContent).toContain("Send as image");
      fireEvent.click(screen.getByRole("button", { name: "Share as file" }));
      await waitFor(() => {
        expect(add).toHaveBeenCalledWith("/tmp/cat.png");
      });
      expect(read).not.toHaveBeenCalled();
      expect(screen.queryByRole("dialog")).toBeNull();
    });

    it("sends images inline on request and shares the rest of a mixed drop", async () => {
      const add = vi.spyOn(api, "filesChipAdd").mockResolvedValue(chip(1));
      const read = vi
        .spyOn(api, "readDroppedImage")
        .mockResolvedValue({ name: "cat.png", mime: "image/png", data: btoa("x") });
      setup(CHANNEL);
      emit({ type: "drop", paths: ["/tmp/cat.png", "/tmp/a.pdf"], position: at(100, 100) });
      const inline = await screen.findByRole("button", { name: "Send as image" });
      expect(document.activeElement).toBe(inline);
      fireEvent.click(inline);
      await waitFor(() => {
        expect(read).toHaveBeenCalledWith("/tmp/cat.png");
        expect(add).toHaveBeenCalledWith("/tmp/a.pdf");
      });
      expect(add).not.toHaveBeenCalledWith("/tmp/cat.png");
    });

    it("does nothing when cancelled with Escape", async () => {
      const add = vi.spyOn(api, "filesChipAdd");
      const read = vi.spyOn(api, "readDroppedImage");
      setup(CHANNEL);
      emit({ type: "drop", paths: ["/tmp/cat.png"], position: at(100, 100) });
      await screen.findByRole("dialog");
      fireEvent.keyDown(document, { key: "Escape" });
      expect(screen.queryByRole("dialog")).toBeNull();
      expect(add).not.toHaveBeenCalled();
      expect(read).not.toHaveBeenCalled();
    });
  });
});
