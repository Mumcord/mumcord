import { describe, expect, it } from "vitest";
import { installLinkGuard } from "./nativeLinks";

const LINK = '<a id="l" href="https://mumcord.app/page">link</a><p id="p">text</p>';

function mouse(type: string, target: Element, init: MouseEventInit): MouseEvent {
  const event = new MouseEvent(type, { bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event);
  return event;
}

/** Installs the guard with a recording opener and runs `act` on the page from `html`. */
function guard(html: string, act: (opened: string[], body: HTMLElement) => void): string[] {
  const opened: string[] = [];
  const off = installLinkGuard(document, (url) => opened.push(url));
  document.body.innerHTML = html;
  try {
    act(opened, document.body);
  } finally {
    off();
    document.body.innerHTML = "";
  }
  return opened;
}

describe("link guard", () => {
  it("sends a middle-clicked link to the browser instead of a window of its own", () => {
    const opened = guard(LINK, (seen, body) => {
      const event = mouse("auxclick", body.querySelector("#l") as Element, { button: 1 });
      expect(event.defaultPrevented).toBe(true);
      expect(seen).toEqual(["https://mumcord.app/page"]);
    });
    expect(opened).toEqual(["https://mumcord.app/page"]);
  });

  it("sends a link clicked with Ctrl, Cmd or Shift to the browser", () => {
    for (const modifier of ["ctrlKey", "metaKey", "shiftKey"]) {
      guard(LINK, (seen, body) => {
        const event = mouse("click", body.querySelector("#l") as Element, {
          button: 0,
          [modifier]: true,
        });
        expect(event.defaultPrevented).toBe(true);
        expect(seen).toEqual(["https://mumcord.app/page"]);
      });
    }
  });

  it("leaves plain clicks and clicks a view already handled alone", () => {
    guard(LINK, (seen, body) => {
      const plain = mouse("click", body.querySelector("#l") as Element, { button: 0 });
      expect(plain.defaultPrevented).toBe(false);
      // The chat opens the link itself (its own listener runs first) and handles the click.
      const link = body.querySelector("#l") as Element;
      link.addEventListener("click", (event) => {
        event.preventDefault();
      });
      const handled = mouse("click", link, { button: 0, ctrlKey: true });
      expect(handled.defaultPrevented).toBe(true);
      expect(seen).toEqual([]);
    });
  });

  it("ignores middle clicks that are not on a link", () => {
    guard(LINK, (seen, body) => {
      const event = mouse("auxclick", body.querySelector("#p") as Element, { button: 1 });
      expect(event.defaultPrevented).toBe(false);
      expect(seen).toEqual([]);
    });
  });

  it("stops listening once it is uninstalled", () => {
    const opened: string[] = [];
    const off = installLinkGuard(document, (url) => opened.push(url));
    off();
    document.body.innerHTML = LINK;
    const event = mouse("auxclick", document.getElementById("l") as Element, { button: 1 });
    expect(event.defaultPrevented).toBe(false);
    expect(opened).toEqual([]);
    document.body.innerHTML = "";
  });
});
