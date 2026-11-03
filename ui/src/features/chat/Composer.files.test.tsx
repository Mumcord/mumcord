import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { DragDropEvent } from "@tauri-apps/api/webview";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi } from "../../api";
import type { FileChip } from "../../bindings/commands";
import { chipStatus } from "../files/composer/chipView";
import { connectedMock, resetAll } from "../channels/testUtils";
import { i18n } from "../../i18n";
import { Composer } from "./Composer";

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
  render(
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
    act(() => {
      api.mock.emit("fileChipChanged", { chip: chip(1, { state: { kind: "ready" } }) });
    });
    expect(await screen.findByText("18.4 MB · peer-to-peer")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => {
      expect(send).toHaveBeenCalledWith(expect.any(Number), 1, [1]);
    });
  });
});
