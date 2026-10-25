import { describe, expect, it } from "vitest";
import { installNativeDragGuard, isFileDrag, shouldBlockNativeDrag } from "./nativeDrag";

function dragStart(target: Element): boolean {
  const event = new Event("dragstart", { bubbles: true, cancelable: true });
  target.dispatchEvent(event);
  return event.defaultPrevented;
}

function dragWith(type: "dragover" | "drop", types: string[]): boolean {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(event, "dataTransfer", { value: { types } });
  document.body.dispatchEvent(event);
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

  it("swallows file and link drops anywhere so the webview never navigates", () => {
    const off = installNativeDragGuard();
    for (const type of ["dragover", "drop"] as const) {
      expect(dragWith(type, ["Files"])).toBe(true);
      // WebKitGTK: a file from the file manager arrives only as a file:// link.
      expect(dragWith(type, ["text/uri-list", "text/plain"])).toBe(true);
      expect(dragWith(type, ["text/plain"])).toBe(false);
    }
    off();
    expect(dragWith("drop", ["Files"])).toBe(false);
  });
  it("recognises file drags", () => {
    expect(isFileDrag(null)).toBe(false);
    expect(isFileDrag({ types: ["text/html"] } as unknown as DataTransfer)).toBe(false);
    expect(isFileDrag({ types: ["Files"] } as unknown as DataTransfer)).toBe(true);
  });
});
