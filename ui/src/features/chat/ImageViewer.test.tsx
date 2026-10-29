import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { originalSrc } from "./displaySrc";
import { resetFullImages } from "./fullImage";
import { FullImageBars } from "./FullImageBars";
import { ImageViewer, type ViewerImage } from "./ImageViewer";
import { fitScale, zoomAround } from "./viewerMath";

/** The data: URL the viewer shows; the `<img>` itself gets a blob: URL of it. */
function shownSrc(): string {
  const src = screen.getByRole("img").getAttribute("src") ?? "";
  expect(src).toMatch(/^blob:/);
  return originalSrc(src);
}

const dialogs = vi.hoisted(() => ({ pickSavePath: vi.fn() }));
vi.mock("../settings/fileDialogs", () => dialogs);

const PNG = "data:image/png;base64,iVBORw0KGgo=";
const GIF = "data:image/gif;base64,R0lGODlhAQABAAAAACw=";
const IMAGES: ViewerImage[] = [
  { src: PNG, name: "image" },
  { src: GIF, url: "https://pics.example/cat.gif", name: "cat.gif" },
  { src: PNG, name: "image" },
];

let api: MockApi;
const frames: FrameRequestCallback[] = [];

/** Runs the animation frames the viewer asked for (one per burst of events). */
function runFrames() {
  act(() => {
    for (const frame of frames.splice(0)) frame(0);
  });
}

beforeAll(async () => {
  await applyLanguage("en");
  Element.prototype.setPointerCapture = vi.fn();
});

beforeEach(() => {
  api = createMockApi();
  setApi(api);
  dialogs.pickSavePath.mockReset();
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => frames.push(callback));
  vi.stubGlobal("cancelAnimationFrame", () => undefined);
});

afterEach(() => {
  vi.unstubAllGlobals();
  setApi(undefined);
});

function Harness({ onClose = () => undefined }: { onClose?: () => void }) {
  const [index, setIndex] = useState(0);
  return <ImageViewer images={IMAGES} index={index} onIndexChange={setIndex} onClose={onClose} />;
}

const dialog = () => screen.getByRole("dialog", { name: "Image viewer" });
const transform = () => {
  runFrames();
  return screen.getByRole("img").style.transform;
};

describe("zoom math", () => {
  const stage = { width: 800, height: 600 };
  const image = { width: 400, height: 300 };

  it("fits the whole image into the stage", () => {
    expect(fitScale(stage, image)).toBe(2);
    expect(fitScale(stage, { width: 1600, height: 300 })).toBe(0.5);
    // A tiny image is not blown up beyond a sensible factor.
    expect(fitScale(stage, { width: 1, height: 1 })).toBe(4);
  });

  it("keeps the image point under the pointer fixed while zooming", () => {
    const pointer = { x: 100, y: -50 };
    const before = { scale: 1, x: 20, y: 10 };
    const after = zoomAround(before, 2, pointer, stage, image);
    expect(after.scale).toBe(2);
    // The image point under the pointer: (pointer - offset) / scale, identical before and after.
    expect((pointer.x - after.x) / after.scale).toBeCloseTo((pointer.x - before.x) / before.scale);
    expect((pointer.y - after.y) / after.scale).toBeCloseTo((pointer.y - before.y) / before.scale);
  });
});

describe("ImageViewer", () => {
  it("is a modal dialog with a counter, focus inside and Escape closing it", async () => {
    const onClose = vi.fn();
    const { baseElement } = render(<Harness onClose={onClose} />);
    expect(dialog()).toHaveFocus();
    expect(screen.getByText("1 / 3")).toBeInTheDocument();
    expect(await axeViolations(baseElement)).toEqual([]);
    fireEvent.keyDown(dialog(), { key: "Escape" });
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("closes on a click on the backdrop but not on the image", () => {
    const onClose = vi.fn();
    render(<Harness onClose={onClose} />);
    fireEvent.click(screen.getByRole("img"));
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(document.querySelector(".image-viewer__stage") as HTMLElement);
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("zooms with + and −, fits with 0 and toggles 100 % on double-click", () => {
    render(<Harness />);
    expect(transform()).toContain("scale(1)");
    fireEvent.keyDown(dialog(), { key: "+" });
    expect(transform()).toContain("scale(1.25)");
    fireEvent.keyDown(dialog(), { key: "-" });
    fireEvent.keyDown(dialog(), { key: "-" });
    expect(transform()).toContain("scale(0.8)");
    fireEvent.keyDown(dialog(), { key: "0" });
    expect(transform()).toContain("scale(1)");
    fireEvent.keyDown(dialog(), { key: "+" });
    fireEvent.doubleClick(document.querySelector(".image-viewer__stage") as HTMLElement);
    expect(transform()).toContain("scale(1)");
  });

  it("zooms with the wheel and with a touchpad pinch (ctrl + wheel)", () => {
    render(<Harness />);
    const stage = document.querySelector(".image-viewer__stage") as HTMLElement;
    fireEvent.wheel(stage, { deltaY: -100 });
    const wheeled = Number(/scale\(([\d.]+)\)/.exec(transform())?.[1]);
    expect(wheeled).toBeGreaterThan(1);
    fireEvent.wheel(stage, { deltaY: -5, ctrlKey: true });
    expect(Number(/scale\(([\d.]+)\)/.exec(transform())?.[1])).toBeGreaterThan(wheeled);
  });

  it("revokes the blob: URL of each image it stops showing and when it closes", () => {
    const revoke = vi.spyOn(URL, "revokeObjectURL");
    const { unmount } = render(<Harness />);
    const first = screen.getByRole("img").getAttribute("src");
    fireEvent.keyDown(dialog(), { key: "ArrowRight" });
    expect(revoke).toHaveBeenCalledWith(first);
    const second = screen.getByRole("img").getAttribute("src");
    unmount();
    expect(revoke).toHaveBeenCalledWith(second);
    revoke.mockRestore();
  });

  it("steps through the images with the arrow keys and the buttons", () => {
    render(<Harness />);
    fireEvent.keyDown(dialog(), { key: "ArrowRight" });
    expect(screen.getByText("2 / 3")).toBeInTheDocument();
    expect(shownSrc()).toBe(GIF);
    fireEvent.keyDown(dialog(), { key: "ArrowLeft" });
    expect(screen.getByText("1 / 3")).toBeInTheDocument();
    fireEvent.keyDown(dialog(), { key: "ArrowLeft" });
    expect(screen.getByText("1 / 3")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Next image" }));
    expect(screen.getByText("2 / 3")).toBeInTheDocument();
  });

  it("keeps Tab inside the dialog", () => {
    render(<Harness />);
    const buttons = screen.getAllByRole("button");
    const first = buttons[0] as HTMLElement;
    const last = buttons[buttons.length - 1] as HTMLElement;
    last.focus();
    fireEvent.keyDown(last, { key: "Tab" });
    expect(first).toHaveFocus();
    fireEvent.keyDown(first, { key: "Tab", shiftKey: true });
    expect(last).toHaveFocus();
  });

  it("saves the loaded image through the engine, without a new download", async () => {
    dialogs.pickSavePath.mockResolvedValue("/tmp/pic.png");
    const save = vi.spyOn(api, "saveImage");
    render(<Harness />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Save image" }));
      await Promise.resolve();
    });
    expect(dialogs.pickSavePath).toHaveBeenCalledWith(
      "image.png",
      expect.objectContaining({ extensions: ["png"] }),
      "Save image",
    );
    expect(save).toHaveBeenCalledWith("/tmp/pic.png", PNG);
    expect(await screen.findByText("Image saved")).toBeInTheDocument();
  });

  it("saves nothing when the save dialog is cancelled", async () => {
    dialogs.pickSavePath.mockResolvedValue(null);
    const save = vi.spyOn(api, "saveImage");
    render(<Harness />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Save image" }));
      await Promise.resolve();
    });
    expect(save).not.toHaveBeenCalled();
  });

  it("reports a failed save", async () => {
    dialogs.pickSavePath.mockResolvedValue("/tmp/pic.png");
    vi.spyOn(api, "saveImage").mockRejectedValue(new Error("disk full"));
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    render(<Harness />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Save image" }));
      await Promise.resolve();
    });
    expect(await screen.findByRole("alert")).toHaveTextContent("could not be saved");
  });

  it("offers Open in browser only for external images", () => {
    const openLink = vi.spyOn(api, "openLink").mockResolvedValue(null);
    render(<Harness />);
    expect(screen.queryByRole("button", { name: "Open in browser" })).toBeNull();
    fireEvent.keyDown(dialog(), { key: "ArrowRight" });
    fireEvent.click(screen.getByRole("button", { name: "Open in browser" }));
    expect(openLink).toHaveBeenCalledWith("https://pics.example/cat.gif");
  });
});

describe("full images", () => {
  const REFERENCE = `${"cd".repeat(32)}:2048`;
  const FULL = "data:image/png;base64,RlVMTA==";
  const thumbnail: ViewerImage = {
    src: PNG,
    name: "image",
    full: { reference: REFERENCE, senderSession: 4, senderCertHash: "cert" },
  };

  beforeEach(() => {
    resetFullImages();
  });

  function viewer() {
    return render(
      <ImageViewer images={[thumbnail]} index={0} onIndexChange={vi.fn()} onClose={vi.fn()} />,
    );
  }

  function deferredFetch() {
    let resolve: (src: string | null) => void = () => undefined;
    const fetch = vi.spyOn(api, "fullImageFetch").mockImplementation(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    return {
      fetch,
      resolve: (src: string | null) => {
        resolve(src);
      },
    };
  }

  it("shows the thumbnail with progress, then the full image", async () => {
    const { fetch, resolve } = deferredFetch();
    viewer();
    await waitFor(() => {
      expect(fetch).toHaveBeenCalledWith(expect.any(Number), REFERENCE, 4, "cert");
    });
    expect(shownSrc()).toBe(PNG);
    expect(screen.getByRole("status")).toHaveTextContent("Loading full size…");
    expect(screen.getByText("Beta")).toBeInTheDocument();
    act(() => {
      api.mock.emit("fullImageProgress", {
        serverId: 1,
        reference: REFERENCE,
        received: 512,
        total: 2048,
      });
    });
    expect(screen.getByRole("status")).toHaveTextContent("Loading full size… 25 %");
    resolve(FULL);
    await waitFor(() => {
      expect(shownSrc()).toBe(FULL);
    });
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("keeps the transfer running when the viewer closes (FR-2.38)", async () => {
    const { fetch } = deferredFetch();
    const cancel = vi.spyOn(api, "fullImageCancel");
    const { unmount } = viewer();
    await waitFor(() => {
      expect(fetch).toHaveBeenCalled();
    });
    unmount();
    await new Promise((done) => setTimeout(done, 10));
    expect(cancel).not.toHaveBeenCalled();
  });

  it("reopening the viewer attaches to the same transfer with its progress", async () => {
    const { fetch, resolve } = deferredFetch();
    const first = viewer();
    await waitFor(() => {
      expect(fetch).toHaveBeenCalled();
    });
    act(() => {
      api.mock.emit("fullImageProgress", {
        serverId: 1,
        reference: REFERENCE,
        received: 1024,
        total: 2048,
      });
    });
    first.unmount();
    viewer();
    expect(await screen.findByRole("status")).toHaveTextContent("Loading full size… 50 %");
    expect(fetch).toHaveBeenCalledTimes(1);
    resolve(FULL);
    await waitFor(() => {
      expect(shownSrc()).toBe(FULL);
    });
  });

  it("opens a transfer that finished while the viewer was closed at once", async () => {
    const { fetch, resolve } = deferredFetch();
    const first = viewer();
    await waitFor(() => {
      expect(fetch).toHaveBeenCalled();
    });
    first.unmount();
    resolve(FULL);
    await new Promise((done) => setTimeout(done, 10));
    viewer();
    await waitFor(() => {
      expect(shownSrc()).toBe(FULL);
    });
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("cancels on purpose with the x on the progress bar (FR-2.38)", async () => {
    const { fetch } = deferredFetch();
    const cancel = vi.spyOn(api, "fullImageCancel");
    const html = `<img src="${PNG}" data-mumcord-full="${REFERENCE}">`;
    render(<FullImageBars html={html} />);
    viewer();
    await waitFor(() => {
      expect(fetch).toHaveBeenCalled();
    });
    const button = await screen.findByRole("button", { name: "Cancel full-size download" });
    fireEvent.click(button);
    expect(cancel).toHaveBeenCalledWith(expect.any(Number), REFERENCE);
  });

  it("does not cancel a transfer that already finished", async () => {
    const { fetch, resolve } = deferredFetch();
    const cancel = vi.spyOn(api, "fullImageCancel");
    const { unmount } = viewer();
    await waitFor(() => {
      expect(fetch).toHaveBeenCalled();
    });
    resolve(FULL);
    await waitFor(() => {
      expect(shownSrc()).toBe(FULL);
    });
    unmount();
    await new Promise((done) => setTimeout(done, 10));
    expect(cancel).not.toHaveBeenCalled();
  });

  it("shows a progress bar under the chat image while it loads (FR-2.22)", async () => {
    const { fetch, resolve } = deferredFetch();
    const html = `<img src="${PNG}" data-mumcord-full="${REFERENCE}">`;
    render(<FullImageBars html={html} />);
    expect(screen.queryByRole("progressbar")).toBeNull();
    viewer();
    await waitFor(() => {
      expect(fetch).toHaveBeenCalled();
    });
    const bar = await screen.findByRole("progressbar", { name: "Loading full size…" });
    act(() => {
      api.mock.emit("fullImageProgress", {
        serverId: 1,
        reference: REFERENCE,
        received: 512,
        total: 2048,
      });
    });
    expect(bar).toHaveAttribute("aria-valuenow", "25");
    resolve(FULL);
    await waitFor(() => {
      expect(screen.queryByRole("progressbar")).toBeNull();
    });
  });

  it("keeps the thumbnail when the full image cannot be had", async () => {
    const { fetch, resolve } = deferredFetch();
    viewer();
    await waitFor(() => {
      expect(fetch).toHaveBeenCalled();
    });
    resolve(null);
    await waitFor(() => {
      expect(screen.getByRole("status")).toHaveTextContent("Full size not available");
    });
    expect(shownSrc()).toBe(PNG);
  });

  it("saves the full image once it is there", async () => {
    vi.spyOn(api, "fullImageFetch").mockResolvedValue(FULL);
    dialogs.pickSavePath.mockResolvedValue("/tmp/pic.png");
    const save = vi.spyOn(api, "saveImage");
    viewer();
    await act(async () => {
      await Promise.resolve();
    });
    expect(shownSrc()).toBe(FULL);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Save image" }));
      await Promise.resolve();
    });
    expect(save).toHaveBeenCalledWith("/tmp/pic.png", FULL);
  });

  it("never requests the original while loading full images is off (FR-1.74)", async () => {
    await api.settingsUpdate({ ...(await api.settingsGet()), loadFullImages: false });
    const fetch = vi.spyOn(api, "fullImageFetch");
    viewer();
    await waitFor(() => {
      expect(screen.queryByRole("status")).toBeNull();
    });
    expect(fetch).not.toHaveBeenCalled();
  });

  it("does not fetch anything for images without a full version", () => {
    const fetch = vi.spyOn(api, "fullImageFetch");
    render(<Harness />);
    expect(fetch).not.toHaveBeenCalled();
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.queryByText("Beta")).toBeNull();
  });
});
