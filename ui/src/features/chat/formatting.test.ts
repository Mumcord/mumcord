import { describe, expect, it } from "vitest";
import { applyFormat, formatForKey, type Edit, type Format } from "./formatting";

/** `|` marks the caret, `[` … `]` a selection. */
function edit(marked: string): Edit {
  const start = marked.search(/[|[]/);
  const caret = marked[start] === "|";
  const text = marked.replace(caret ? "|" : /[[\]]/g, "");
  const end = caret ? start : marked.indexOf("]") - 1;
  return { text, selectionStart: start, selectionEnd: end };
}

function show({ text, selectionStart, selectionEnd }: Edit): string {
  if (selectionStart === selectionEnd)
    return text.slice(0, selectionStart) + "|" + text.slice(selectionStart);
  return `${text.slice(0, selectionStart)}[${text.slice(selectionStart, selectionEnd)}]${text.slice(selectionEnd)}`;
}

const apply = (marked: string, format: Format) => show(applyFormat(edit(marked), format));

describe("applyFormat", () => {
  it("wraps the selection and keeps it selected", () => {
    expect(apply("say [hi] now", "bold")).toBe("say **[hi]** now");
    expect(apply("say [hi] now", "italic")).toBe("say *[hi]* now");
    expect(apply("say [hi] now", "strike")).toBe("say ~~[hi]~~ now");
    expect(apply("run [ls] now", "code")).toBe("run `[ls]` now");
  });

  it("inserts empty markers with the caret between them", () => {
    expect(apply("a |", "bold")).toBe("a **|**");
    expect(apply("|", "italic")).toBe("*|*");
    expect(apply("x|y", "code")).toBe("x`|`y");
  });

  it("toggles a quote on the current line", () => {
    expect(apply("one\ntw|o\nthree", "quote")).toBe("one\n> tw|o\nthree");
    expect(apply("one\n> tw|o\nthree", "quote")).toBe("one\ntw|o\nthree");
  });

  it("toggles a quote on every selected line", () => {
    expect(apply("a[a\nbb\nc]c\nd", "quote")).toBe("> a[a\n> bb\n> c]c\nd");
    expect(apply("> a[a\n> bb\n> c]c\nd", "quote")).toBe("a[a\nbb\nc]c\nd");
  });

  it("quotes all lines when only some are quoted", () => {
    expect(apply("[> a\nb]", "quote")).toBe("> [> a\n> b]");
  });

  it("puts a code block on its own lines", () => {
    expect(apply("|", "codeBlock")).toBe("```\n|\n```");
    expect(apply("look: [let a = 1] ok", "codeBlock")).toBe("look: \n```\n[let a = 1]\n```\n ok");
    expect(apply("one\n[two]", "codeBlock")).toBe("one\n```\n[two]\n```");
  });

  it("turns a selection into a link and places the caret in the address", () => {
    expect(apply("see [docs]", "link")).toBe("see [docs](|)");
    expect(apply("|", "link")).toBe("[|]()");
    expect(apply("[https://example.com]", "link")).toBe("[|](https://example.com)");
  });
});

describe("formatForKey", () => {
  const key = (k: string, mods: Partial<KeyboardEventInit> = {}) =>
    formatForKey({
      key: k,
      ctrlKey: false,
      metaKey: false,
      shiftKey: false,
      altKey: false,
      ...mods,
    });

  it("maps the documented shortcuts", () => {
    expect(key("b", { ctrlKey: true })).toBe("bold");
    expect(key("i", { metaKey: true })).toBe("italic");
    expect(key("X", { ctrlKey: true, shiftKey: true })).toBe("strike");
    expect(key("e", { ctrlKey: true })).toBe("code");
    expect(key("E", { ctrlKey: true, shiftKey: true })).toBe("codeBlock");
    expect(key("k", { ctrlKey: true })).toBe("link");
  });

  it("ignores other keys and modifiers", () => {
    expect(key("b")).toBeNull();
    expect(key("b", { ctrlKey: true, altKey: true })).toBeNull();
    expect(key("x", { ctrlKey: true })).toBeNull();
    expect(key("k", { ctrlKey: true, shiftKey: true })).toBeNull();
  });
});

describe("quote shortcut", () => {
  it("maps Ctrl+Shift+9 and Cmd+Shift+9 to quote", () => {
    const base = { ctrlKey: false, metaKey: false, shiftKey: true, altKey: false };
    expect(formatForKey({ ...base, ctrlKey: true, key: "(" })).toBe("quote");
    expect(formatForKey({ ...base, metaKey: true, key: "9" })).toBe("quote");
  });
});
