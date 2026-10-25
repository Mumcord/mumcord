import { describe, expect, it } from "vitest";
import { installNativeDragGuard, shouldBlockNativeDrag } from "./nativeDrag";

function dragStart(target: Element): boolean {
  const event = new Event("dragstart", { bubbles: true, cancelable: true });
  target.dispatchEvent(event);
  return event.defaultPrevented;
}

describe("native drag guard", () => {
  it("prevents dragging images and data: links", () => {
    const off = installNativeDragGuard();
    document.body.innerHTML =
      '<img id="i" src="data:image/gif;base64,AAAA"><a id="a" href="data:text/plain,x">x</a>';
    expect(dragStart(document.getElementById("i") as Element)).toBe(true);
    expect(dragStart(document.getElementById("a") as Element)).toBe(true);
    off();
  });
  it("keeps inputs and plain text draggable", () => {
    const off = installNativeDragGuard();
    document.body.innerHTML =
      '<input id="n"><p id="p">text</p><a id="l" href="https://x.test">l</a>';
    expect(dragStart(document.getElementById("n") as Element)).toBe(false);
    expect(dragStart(document.getElementById("p") as Element)).toBe(false);
    expect(dragStart(document.getElementById("l") as Element)).toBe(false);
    off();
  });
  it("ignores non-element targets", () => {
    expect(shouldBlockNativeDrag(null)).toBe(false);
  });
});
