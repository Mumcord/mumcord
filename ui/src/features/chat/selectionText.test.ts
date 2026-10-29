import { afterEach, describe, expect, it } from "vitest";
import { selectedText } from "./selectionText";

function selectAll(html: string): HTMLElement {
  const root = document.createElement("div");
  root.innerHTML = html;
  document.body.append(root);
  const range = document.createRange();
  range.selectNodeContents(root);
  const selection = window.getSelection();
  selection?.removeAllRanges();
  selection?.addRange(range);
  return root;
}

afterEach(() => {
  window.getSelection()?.removeAllRanges();
  document.body.replaceChildren();
});

describe("selectedText", () => {
  it("keeps line breaks between blocks, of <br> and inside <pre>", () => {
    const root = selectAll(
      "<p>one<br>two</p>\n  <div>three   words</div><pre>a\n  b</pre><ul><li>x</li><li>y</li></ul>",
    );
    expect(selectedText(window.getSelection(), root)).toBe("one\ntwo\nthree words\na\n  b\nx\ny");
  });

  it("leaves out screen-reader labels, hidden hover times and action buttons", () => {
    const root = selectAll(
      '<li><span class="visually-hidden">Maya </span>' +
        '<div class="chat-message__gutter"><time class="chat-message__hover-time">11:31</time></div>' +
        '<span class="avatar" aria-hidden="true">M</span>' +
        '<div class="chat-message__body">second</div>' +
        '<div class="chat-message__actions"><button>Reply</button></div></li>' +
        '<li><div class="chat-message__header"><span>Jonas</span> <time class="chat-message__time">11:32</time></div>' +
        '<div class="chat-message__body"><pre><code>x</code></pre><div class="chat-code__actions"><button>Copy code</button></div></div></li>',
    );
    expect(selectedText(window.getSelection(), root)).toBe("second\nJonas 11:32\nx");
  });

  it("copies only the selected part of a text", () => {
    const root = selectAll("<div>hello world</div>");
    const text = root.querySelector("div")?.firstChild as Text;
    const range = document.createRange();
    range.setStart(text, 6);
    range.setEnd(text, 11);
    window.getSelection()?.removeAllRanges();
    window.getSelection()?.addRange(range);
    expect(selectedText(window.getSelection(), root)).toBe("world");
  });

  it("keeps the spacing of code when the selection lies inside one block", () => {
    const root = selectAll("<pre><code>a\n  b\n</code></pre>");
    const text = root.querySelector("code")?.firstChild as Text;
    const range = document.createRange();
    range.setStart(text, 0);
    range.setEnd(text, 5);
    window.getSelection()?.removeAllRanges();
    window.getSelection()?.addRange(range);
    expect(selectedText(window.getSelection(), root)).toBe("a\n  b");
  });

  it("is empty without a selection or for a selection outside the root", () => {
    const root = selectAll("<div>inside</div>");
    const outside = document.createElement("p");
    outside.textContent = "outside";
    document.body.append(outside);
    const range = document.createRange();
    range.selectNodeContents(outside);
    window.getSelection()?.removeAllRanges();
    window.getSelection()?.addRange(range);
    expect(selectedText(window.getSelection(), root)).toBe("");
    window.getSelection()?.removeAllRanges();
    expect(selectedText(window.getSelection(), root)).toBe("");
  });
});
