import { act, render, screen, waitFor } from "@testing-library/react";
import type { DragDropEvent } from "@tauri-apps/api/webview";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { EngineError, type MockApi } from "../../api";
import { connectedMock, resetAll } from "../channels/testUtils";
import { Composer } from "./Composer";

const prepare = vi.hoisted(() => vi.fn());
vi.mock("./images", async (original) => ({
  ...(await original<typeof import("./images")>()),
  prepareImage: prepare,
}));

// Stands in for Tauri's native drag-and-drop events.
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

// PhysicalPosition is a class in @tauri-apps/api; the handler only reads x and y.
function at(x: number, y: number) {
  return { x, y } as Extract<DragDropEvent, { type: "over" }>["position"];
}

let api: MockApi;

beforeEach(async () => {
  prepare.mockReset();
  prepare.mockImplementation((file: File) =>
    Promise.resolve({
      ok: true,
      dataUrl: `data:image/png;base64,${file.name}`,
      width: 10,
      height: 10,
      scaled: false,
    }),
  );
  api = await connectedMock();
  Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
  Object.defineProperty(window, "devicePixelRatio", { value: 2, configurable: true });
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

function setup() {
  const { container } = render(
    <Composer
      target={{ kind: "channel", channelId: 1 }}
      limits={{ text: 5000, image: 100_000, allowHtml: true }}
      label="Message #Lobby"
      blockedReason={null}
    />,
  );
  return container.querySelector("form") as HTMLFormElement;
}

describe("Composer native file drops", () => {
  it("highlights while a file is dragged over it and attaches the dropped image", async () => {
    const read = vi
      .spyOn(api, "readDroppedImage")
      .mockResolvedValue({ name: "cat.png", mime: "image/png", data: btoa("x") });
    const form = setup();
    await waitFor(() => {
      expect(dropHandlers.size).toBe(1);
    });
    // Physical pixels at scale 2: (100, 1500) is (50, 750) in CSS pixels, inside the composer.
    emit({ type: "over", position: at(100, 1500) });
    expect(form.dataset.dragging).toBe("true");
    emit({ type: "over", position: at(100, 200) });
    expect(form.dataset.dragging).toBeUndefined();
    emit({ type: "drop", paths: ["/home/me/cat.png"], position: at(100, 1500) });
    expect(form.dataset.dragging).toBeUndefined();
    expect(await screen.findByRole("img", { name: "Image to send: cat.png" })).toBeTruthy();
    expect(read).toHaveBeenCalledWith("/home/me/cat.png");
  });

  it("ignores drops elsewhere in the window", async () => {
    const read = vi.spyOn(api, "readDroppedImage");
    setup();
    await waitFor(() => {
      expect(dropHandlers.size).toBe(1);
    });
    emit({ type: "drop", paths: ["/home/me/cat.png"], position: at(100, 200) });
    expect(read).not.toHaveBeenCalled();
  });

  it("shows an error when a dropped file is not an image", async () => {
    vi.spyOn(api, "readDroppedImage").mockRejectedValue(new EngineError("invalidInput", "no"));
    setup();
    await waitFor(() => {
      expect(dropHandlers.size).toBe(1);
    });
    emit({ type: "drop", paths: ["/home/me/notes.txt"], position: at(100, 1500) });
    expect((await screen.findByRole("alert")).textContent).toBe("Only images can be sent.");
  });
});
