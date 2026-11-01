import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
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

/** A paste as WebKitGTK delivers a file copied in the file manager: the paths stay hidden. */
function pasteUriList(input: HTMLElement, uriList = "") {
  return fireEvent.paste(input, {
    clipboardData: {
      files: [],
      items: [{ kind: "string", type: "text/uri-list", getAsFile: () => null }],
      types: ["text/uri-list"],
      getData: (type: string) => (type === "text/uri-list" ? uriList : ""),
    },
  });
}

describe("Composer paste of copied image files (issue #50)", () => {
  it("attaches an image file copied in the file manager", async () => {
    const copied = vi.spyOn(api, "clipboardFiles").mockResolvedValue(["/home/me/cat.png"]);
    const read = vi
      .spyOn(api, "readDroppedImage")
      .mockResolvedValue({ name: "cat.png", mime: "image/png", data: btoa("x") });
    setup();
    const notPrevented = pasteUriList(screen.getByRole("textbox"));
    expect(notPrevented).toBe(false);
    expect(await screen.findByRole("img", { name: "Image to send: cat.png" })).toBeTruthy();
    expect(copied).toHaveBeenCalledOnce();
    expect(read).toHaveBeenCalledWith("/home/me/cat.png");
  });

  it("shows the drop error when the copied file is not an image", async () => {
    vi.spyOn(api, "clipboardFiles").mockResolvedValue(["/home/me/notes.txt"]);
    vi.spyOn(api, "readDroppedImage").mockRejectedValue(new EngineError("invalidInput", "no"));
    setup();
    pasteUriList(screen.getByRole("textbox"));
    expect((await screen.findByRole("alert")).textContent).toBe("Only images can be sent.");
  });

  it("says so when the copied files cannot be read from the clipboard", async () => {
    vi.spyOn(api, "clipboardFiles").mockRejectedValue(new EngineError("io", "no"));
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    setup();
    pasteUriList(screen.getByRole("textbox"));
    expect((await screen.findByRole("alert")).textContent).toBe(
      "The image couldn't be read. Try another file.",
    );
  });

  it("uses the files when the webview hands them over", async () => {
    const copied = vi.spyOn(api, "clipboardFiles");
    setup();
    fireEvent.paste(screen.getByRole("textbox"), {
      clipboardData: {
        files: [new File(["x"], "dog.png", { type: "image/png" })],
        types: ["text/uri-list", "Files"],
        getData: () => "",
      },
    });
    expect(await screen.findByRole("img", { name: "Image to send: dog.png" })).toBeTruthy();
    expect(copied).not.toHaveBeenCalled();
  });

  it("pastes a copied web link as text", () => {
    const copied = vi.spyOn(api, "clipboardFiles");
    setup();
    const notPrevented = pasteUriList(screen.getByRole("textbox"), "https://example.com/\r\n");
    expect(notPrevented).toBe(true);
    expect(copied).not.toHaveBeenCalled();
  });
});

type Offer = { kind: "file"; file: File } | { kind: "string"; type: string; value: string };

/**
 * A paste as WebKitGTK hands it to the page. A newer WebKitGTK gives clipboard image data as a
 * file item; the one in the AppImage gives no items at all for it (issue #110).
 */
function paste(input: HTMLElement, offers: readonly Offer[]) {
  const strings = offers.filter((offer) => offer.kind === "string");
  const types = strings.map((offer) => offer.type);
  if (offers.some((offer) => offer.kind === "file")) types.push("Files");
  return fireEvent.paste(input, {
    clipboardData: {
      files: [],
      items: offers.map((offer) =>
        offer.kind === "file"
          ? { kind: "file", type: offer.file.type, getAsFile: () => offer.file }
          : { kind: "string", type: offer.type, getAsFile: () => null },
      ),
      types,
      getData: (type: string) => strings.find((offer) => offer.type === type)?.value ?? "",
    },
  });
}

const screenshot = (): Offer => ({
  kind: "file",
  file: new File(["png"], "image.png", { type: "image/png" }),
});

describe("Composer paste of image data (issue #110)", () => {
  it.each<[string, Offer[]]>([
    ["image data only", [screenshot()]],
    [
      "image data with a file reference",
      [screenshot(), { kind: "string", type: "text/uri-list", value: "file:///tmp/shot.png\r\n" }],
    ],
    [
      "a Spectacle screenshot",
      [
        screenshot(),
        { kind: "string", type: "application/x-kde-suggestedfilename", value: "Screenshot.png" },
        { kind: "string", type: "application/x-kde-onlyReplaceEmpty", value: "1" },
        { kind: "string", type: "text/uri-list", value: "file:///tmp/Screenshot_1.png\r\n" },
      ],
    ],
    [
      "an image copied in a browser",
      [
        screenshot(),
        { kind: "string", type: "text/html", value: '<img src="https://a.test/x.png">' },
      ],
    ],
  ])("attaches %s once, without asking the engine", async (_, offers) => {
    const copied = vi.spyOn(api, "clipboardFiles");
    const native = vi.spyOn(api, "clipboardImage");
    setup();
    expect(paste(screen.getByRole("textbox"), offers)).toBe(false);
    expect(await screen.findByRole("img", { name: "Image to send: image.png" })).toBeTruthy();
    expect(screen.getAllByRole("img", { name: /^Image to send/ })).toHaveLength(1);
    expect(copied).not.toHaveBeenCalled();
    expect(native).not.toHaveBeenCalled();
  });

  it("reads the files of a file reference without image data", async () => {
    vi.spyOn(api, "clipboardFiles").mockResolvedValue(["/home/me/cat.png"]);
    vi.spyOn(api, "readDroppedImage").mockResolvedValue({
      name: "cat.png",
      mime: "image/png",
      data: btoa("x"),
    });
    const native = vi.spyOn(api, "clipboardImage");
    setup();
    paste(screen.getByRole("textbox"), [
      { kind: "string", type: "text/uri-list", value: "" },
      { kind: "string", type: "application/x-kde-cutselection", value: "0" },
    ]);
    expect(await screen.findByRole("img", { name: "Image to send: cat.png" })).toBeTruthy();
    expect(native).not.toHaveBeenCalled();
  });

  it.each<[string, Offer[]]>([
    ["a paste the webview leaves empty", []],
    [
      "an image the webview hands over as an empty file",
      [{ kind: "file", file: new File([], "image.png", { type: "image/png" }) }],
    ],
    [
      "a Spectacle screenshot whose file is gone",
      [{ kind: "string", type: "text/uri-list", value: "file:///tmp/Screenshot_1.png\r\n" }],
    ],
    [
      "an image copied in a browser without its data",
      [{ kind: "string", type: "text/html", value: '<img src="https://a.test/x.png">' }],
    ],
  ])("reads the image data natively for %s", async (_, offers) => {
    vi.spyOn(api, "clipboardFiles").mockResolvedValue([]);
    const native = vi
      .spyOn(api, "clipboardImage")
      .mockResolvedValue({ name: "image.png", mime: "image/png", data: btoa("png") });
    setup();
    // Without a listed image the webview's own paste runs first; the native read follows only
    // because it inserted no text.
    const listed = offers.some((offer) => offer.kind === "file" || offer.type === "text/uri-list");
    expect(paste(screen.getByRole("textbox"), offers)).toBe(!listed);
    expect(await screen.findByRole("img", { name: "Image to send: image.png" })).toBeTruthy();
    expect(native).toHaveBeenCalledOnce();
  });

  it("does nothing when the clipboard holds neither files nor an image", async () => {
    vi.spyOn(api, "clipboardFiles").mockResolvedValue([]);
    const native = vi.spyOn(api, "clipboardImage").mockResolvedValue(null);
    setup();
    paste(screen.getByRole("textbox"), []);
    await waitFor(() => {
      expect(native).toHaveBeenCalledOnce();
    });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByRole("img", { name: /^Image to send/ })).toBeNull();
  });

  it("says so when the native image is too large", async () => {
    vi.spyOn(api, "clipboardFiles").mockResolvedValue([]);
    vi.spyOn(api, "clipboardImage").mockRejectedValue(new EngineError("messageTooLarge", "big"));
    setup();
    paste(screen.getByRole("textbox"), []);
    expect((await screen.findByRole("alert")).textContent).toBe(
      "This file is too large to send (over 25 MB).",
    );
  });

  it("pastes plain text as text", () => {
    const copied = vi.spyOn(api, "clipboardFiles");
    const native = vi.spyOn(api, "clipboardImage");
    setup();
    expect(
      paste(screen.getByRole("textbox"), [{ kind: "string", type: "text/plain", value: "hi" }]),
    ).toBe(true);
    expect(copied).not.toHaveBeenCalled();
    expect(native).not.toHaveBeenCalled();
  });
});
