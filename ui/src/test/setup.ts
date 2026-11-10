import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

afterEach(() => {
  cleanup();
});

// jsdom has no layout, so it has no scrollIntoView either.
if (typeof Element !== "undefined") Element.prototype.scrollIntoView = () => undefined;

// jsdom drives requestAnimationFrame from one long-lived interval on the window. Vitest's fake
// timers take that interval over, and switching back to real timers does not give it back: jsdom
// still thinks a rendering update is scheduled, so every later frame is dropped and anything that
// batches per frame (avatars, drawn rows) never catches up. Run each frame from a fresh timer
// instead, so both timer implementations keep animating.
const frames = new Map<number, ReturnType<typeof setTimeout>>();
let lastFrame = 0;
globalThis.requestAnimationFrame = (callback: FrameRequestCallback): number => {
  const handle = ++lastFrame;
  frames.set(
    handle,
    setTimeout(() => {
      frames.delete(handle);
      callback(performance.now());
    }, 16),
  );
  return handle;
};
globalThis.cancelAnimationFrame = (handle: number): void => {
  const timer = frames.get(handle);
  if (timer !== undefined) clearTimeout(timer);
  frames.delete(handle);
};
