import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setHintPreference } from "../../a11y";
import { applyLanguage } from "../../i18n";
import { GifPicker } from "./GifPicker";
import { clearFavorites, saveFavorite } from "./favorites";

const links = vi.hoisted(() => ({ loadInlineImage: vi.fn() }));
vi.mock("./links", async (original) => ({
  ...(await original<typeof import("./links")>()),
  loadInlineImage: links.loadInlineImage,
}));

const SRC = "data:image/gif;base64,R0lGODlhAQABAAAAACw=";
let callbacks: ((entries: { isIntersecting: boolean }[]) => void)[] = [];

describe("GifPicker thumbnails", () => {
  beforeEach(async () => {
    await applyLanguage("en");
    callbacks = [];
    links.loadInlineImage.mockReset();
    links.loadInlineImage.mockResolvedValue({
      url: "u",
      src: SRC,
      width: 1,
      height: 1,
      animated: true,
    });
    vi.stubGlobal(
      "IntersectionObserver",
      class {
        constructor(cb: (entries: { isIntersecting: boolean }[]) => void) {
          callbacks.push(cb);
        }
        observe() {}
        disconnect() {}
      },
    );
    clearFavorites();
    saveFavorite("https://pics.example/a.gif");
    saveFavorite("https://pics.example/b.gif");
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    clearFavorites();
  });

  it("loads only thumbnails in view and never makes them draggable", async () => {
    render(<GifPicker onSend={() => Promise.resolve()} disabled={false} linkPreviews />);
    fireEvent.click(screen.getByRole("button", { name: /favorite/i }));
    expect(links.loadInlineImage).not.toHaveBeenCalled();
    await act(async () => {
      callbacks[0]?.([{ isIntersecting: true }]);
      await Promise.resolve();
    });
    expect(links.loadInlineImage).toHaveBeenCalledTimes(1);
    const img = document.querySelector("img.gif-picker__img");
    expect(img?.getAttribute("draggable")).toBe("false");
    await act(async () => {
      callbacks[0]?.([{ isIntersecting: false }]);
      await Promise.resolve();
    });
    expect(document.querySelector("img.gif-picker__img")).toBeNull();
  });

  it("keeps the keyboard hint off screen while keyboard hints are turned off", () => {
    render(<GifPicker onSend={() => Promise.resolve()} disabled={false} linkPreviews />);
    fireEvent.click(screen.getByRole("button", { name: /favorite/i }));
    const hint = screen.getByText(/Arrow keys move/);
    expect(hint).toHaveClass("gif-picker__hint");
    act(() => {
      setHintPreference("keyboardHints", false);
    });
    expect(hint).toHaveClass("visually-hidden");
    act(() => {
      setHintPreference("keyboardHints", true);
    });
  });
});
