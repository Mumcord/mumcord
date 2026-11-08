import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "../../i18n";
import { axeViolations } from "../../test/axe";
import { prepareImage } from "../chat/images";
import { cleanCommentHtml } from "./commentHtml";
import { RichCommentEditor } from "./RichCommentEditor";
import type { EditorLimits } from "./richEditorModel";

vi.mock("../chat/images", async (original) => ({
  ...(await original<typeof import("../chat/images")>()),
  prepareImage: vi.fn(),
}));

const LIMITS: EditorLimits = { text: 5000, image: 131072 };

let execCommand: ReturnType<typeof vi.fn>;

beforeEach(() => {
  execCommand = vi.fn(() => true);
  // jsdom has no editing commands; the editor's own DOM edits still run for real.
  // eslint-disable-next-line @typescript-eslint/no-deprecated
  document.execCommand = execCommand as unknown as typeof document.execCommand;
});

afterEach(() => {
  vi.mocked(prepareImage).mockReset();
});

function setup(initialHtml = "", limits: EditorLimits = LIMITS) {
  const onChange = vi.fn();
  const view = render(
    <>
      <span id="label">Comment</span>
      <RichCommentEditor
        labelId="label"
        initialHtml={initialHtml}
        loadKey={0}
        disabled={false}
        onChange={onChange}
        purpose="description"
        limits={limits}
      />
    </>,
  );
  const field = screen.getByRole("textbox", { name: "Comment" });
  const toolbar = screen.getByRole("toolbar", { name: "Formatting" });
  return { view, field, toolbar, onChange };
}

/** Puts the caret into the first text node under `element`. */
function caretIn(element: Element, field: HTMLElement) {
  const text = element.firstChild ?? element;
  const range = document.createRange();
  range.setStart(text, 1);
  range.collapse(true);
  const selection = window.getSelection();
  selection?.removeAllRanges();
  selection?.addRange(range);
  fireEvent.keyUp(field);
}

const lastHtml = (onChange: ReturnType<typeof vi.fn>) =>
  String(onChange.mock.calls.at(-1)?.[0] ?? "");

describe("RichCommentEditor toolbar", () => {
  it("shows the formatting at the caret", () => {
    const { field, toolbar } = setup("<p><b>bold</b> plain</p><ul><li><i>item</i></li></ul>");
    const bold = within(toolbar).getByRole("button", { name: "Bold" });
    const italic = within(toolbar).getByRole("button", { name: "Italic" });
    const bullets = within(toolbar).getByRole("button", { name: "Bulleted list" });
    caretIn(field.querySelector("b") as Element, field);
    expect(bold).toHaveAttribute("aria-pressed", "true");
    expect(italic).toHaveAttribute("aria-pressed", "false");
    caretIn(field.querySelector("i") as Element, field);
    expect(bold).toHaveAttribute("aria-pressed", "false");
    expect(italic).toHaveAttribute("aria-pressed", "true");
    expect(bullets).toHaveAttribute("aria-pressed", "true");
  });

  it("names shortcuts in the themed tooltip and runs them from the keyboard", () => {
    const { field, toolbar } = setup("<p>x</p>");
    const bold = within(toolbar).getByRole("button", { name: "Bold" });
    expect(bold).toHaveAttribute("data-tooltip", "Bold");
    expect(bold).toHaveAttribute("data-tooltip-shortcut", "Ctrl+B");
    expect(bold).not.toHaveAttribute("title");
    fireEvent.keyDown(field, { key: "u", ctrlKey: true });
    fireEvent.keyDown(field, { key: "X", ctrlKey: true, shiftKey: true });
    expect(execCommand).toHaveBeenCalledWith("underline", false, undefined);
    expect(execCommand).toHaveBeenCalledWith("strikeThrough", false, undefined);
  });

  it("moves between buttons with the arrow keys", () => {
    const { toolbar } = setup();
    const bold = within(toolbar).getByRole("button", { name: "Bold" });
    expect(bold).toHaveAttribute("tabindex", "0");
    bold.focus();
    fireEvent.keyDown(bold, { key: "ArrowRight" });
    expect(within(toolbar).getByRole("button", { name: "Italic" })).toHaveFocus();
    fireEvent.keyDown(document.activeElement as Element, { key: "End" });
    expect(within(toolbar).getByRole("button", { name: "Source" })).toHaveFocus();
  });

  it("has no accessibility violations", async () => {
    const { view } = setup("<p><b>x</b></p>");
    expect(await axeViolations(view.container)).toEqual([]);
  });
});

describe("RichCommentEditor colour", () => {
  it("marks the colour at the caret and applies a palette colour", async () => {
    const { field, toolbar } = setup('<p><span style="color:#30a46c">green</span></p>');
    caretIn(field.querySelector("span") as Element, field);
    fireEvent.click(within(toolbar).getByRole("button", { name: "Text colour" }));
    const palette = screen.getByRole("dialog", { name: "Text colour" });
    expect(within(palette).getByRole("radio", { name: "Green" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(within(palette).getByRole("radio", { name: "Green" })).toHaveFocus();
    expect(await axeViolations(palette)).toEqual([]);
    fireEvent.click(within(palette).getByRole("radio", { name: "Red" }));
    expect(execCommand).toHaveBeenCalledWith("foreColor", false, "#e5484d");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("takes a custom hex colour", () => {
    const { toolbar } = setup("<p>x</p>");
    fireEvent.click(within(toolbar).getByRole("button", { name: "Text colour" }));
    const hex = screen.getByRole("textbox", { name: "Custom colour (hex)" });
    fireEvent.change(hex, { target: { value: "#ABC" } });
    fireEvent.keyDown(hex, { key: "Enter" });
    expect(execCommand).toHaveBeenCalledWith("foreColor", false, "#aabbcc");
  });

  it("removes the colour with Default colour", () => {
    const { field, toolbar, onChange } = setup(
      '<p>a <span style="color:#30a46c"><b>green</b></span></p>',
    );
    caretIn(field.querySelector("b") as Element, field);
    fireEvent.click(within(toolbar).getByRole("button", { name: "Text colour" }));
    fireEvent.click(screen.getByRole("radio", { name: "Default colour" }));
    expect(lastHtml(onChange)).toBe("<p>a <b>green</b></p>");
  });
});

describe("RichCommentEditor links", () => {
  it("edits an existing link's text and address", () => {
    const { field, toolbar, onChange } = setup('<p>See <a href="https://a.org">old</a></p>');
    caretIn(field.querySelector("a") as Element, field);
    fireEvent.click(within(toolbar).getByRole("button", { name: "Link" }));
    const popover = screen.getByRole("dialog", { name: "Edit link" });
    const address = within(popover).getByRole("textbox", { name: "Address" });
    expect(address).toHaveValue("https://a.org");
    expect(address).toHaveFocus();
    fireEvent.change(within(popover).getByRole("textbox", { name: "Text" }), {
      target: { value: "Wiki" },
    });
    fireEvent.change(address, { target: { value: "wiki.example.org" } });
    fireEvent.click(within(popover).getByRole("button", { name: "Apply" }));
    expect(cleanCommentHtml(lastHtml(onChange))).toContain(
      '<a href="https://wiki.example.org" rel="noopener noreferrer">Wiki</a>',
    );
  });

  it("removes a link but keeps its text", () => {
    const { field, toolbar, onChange } = setup('<p>See <a href="https://a.org">the wiki</a></p>');
    caretIn(field.querySelector("a") as Element, field);
    fireEvent.keyDown(field, { key: "k", ctrlKey: true });
    fireEvent.click(screen.getByRole("button", { name: "Remove link" }));
    expect(lastHtml(onChange)).toBe("<p>See the wiki</p>");
    expect(within(toolbar).getByRole("button", { name: "Link" })).not.toHaveClass(
      "rte__tool--active",
    );
  });

  it("refuses addresses an official client would not open", () => {
    const { toolbar } = setup("<p>x</p>");
    fireEvent.click(within(toolbar).getByRole("button", { name: "Link" }));
    const popover = screen.getByRole("dialog", { name: "Link" });
    expect(within(popover).queryByRole("button", { name: "Remove link" })).toBeNull();
    fireEvent.change(within(popover).getByRole("textbox", { name: "Address" }), {
      target: { value: "javascript:alert(1)" },
    });
    fireEvent.click(within(popover).getByRole("button", { name: "Apply" }));
    expect(within(popover).getByRole("alert")).toHaveTextContent("Enter a web or mail address");
    expect(execCommand).not.toHaveBeenCalledWith("insertHTML", false, expect.anything());
  });

  it("closes the popover with Escape", () => {
    const { toolbar } = setup("<p>x</p>");
    fireEvent.click(within(toolbar).getByRole("button", { name: "Link" }));
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Address" }), { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});

describe("RichCommentEditor images", () => {
  const file = new File(["x"], "photo.png", { type: "image/png" });
  const pick = (view: ReturnType<typeof setup>["view"]) => {
    const input = view.container.querySelector<HTMLInputElement>('input[type="file"]');
    fireEvent.change(input as HTMLInputElement, { target: { files: [file] } });
  };

  it("scales an image down to what is left of the image limit", async () => {
    vi.mocked(prepareImage).mockResolvedValue({
      ok: true,
      dataUrl: "data:image/jpeg;base64,AAAA",
      width: 288,
      height: 120,
      scaled: true,
    });
    const html = "<p>hello</p>";
    const { view } = setup(html, { text: 5000, image: 1000 });
    pick(view);
    expect(await screen.findByText(/Image scaled down to 288 × 120/u)).toBeInTheDocument();
    // The whole HTML has to fit: the text, the tag and the image itself.
    expect(prepareImage).toHaveBeenCalledWith(file, 1000 - html.length - '<img src="" />'.length, [
      "image/png",
      "image/jpeg",
    ]);
    expect(execCommand).toHaveBeenCalledWith(
      "insertHTML",
      false,
      '<img src="data:image/jpeg;base64,AAAA" />',
    );
  });

  it("refuses an image that cannot fit and says why", async () => {
    vi.mocked(prepareImage).mockResolvedValue({ ok: false, reason: "tooLarge" });
    const { view } = setup("<p>hello</p>");
    pick(view);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "This image doesn't fit the server limit, even scaled down.",
    );
    expect(execCommand).not.toHaveBeenCalledWith("insertHTML", false, expect.anything());
  });

  it("does not even try when the text alone fills the limit", async () => {
    const { view } = setup(`<p>${"x".repeat(100)}</p>`, { text: 5000, image: 100 });
    pick(view);
    expect(await screen.findByRole("alert")).toHaveTextContent("doesn't fit the server limit");
    expect(prepareImage).not.toHaveBeenCalled();
  });
});

describe("RichCommentEditor source view", () => {
  const html =
    '<p><b>Hi</b> <font color="#30a46c">green</font> <span style="color:#e5484d">red</span></p>' +
    '<ul><li><a href="https://a.org">link</a></li></ul>';

  it("round-trips cleaned HTML unchanged and disables formatting meanwhile", () => {
    const { field, toolbar, onChange } = setup(html);
    const clean = cleanCommentHtml(html);
    fireEvent.click(within(toolbar).getByRole("button", { name: "Source" }));
    const source = screen.getByRole("textbox", { name: "Comment" });
    expect(source.tagName).toBe("TEXTAREA");
    expect(source).toHaveValue(clean);
    expect(within(toolbar).getByRole("button", { name: "Bold" })).toBeDisabled();
    expect(within(toolbar).getByRole("button", { name: "Text colour" })).toBeDisabled();
    expect(
      screen.getByText("HTML is cleaned when you switch back and when you save"),
    ).toBeVisible();
    fireEvent.click(within(toolbar).getByRole("button", { name: "Source" }));
    expect(field.innerHTML).toBe(clean);
    expect(lastHtml(onChange)).toBe(clean);
    expect(within(toolbar).getByRole("button", { name: "Bold" })).toBeEnabled();
  });

  it("cleans edited HTML when switching back", () => {
    const { field, toolbar } = setup("<p>x</p>");
    fireEvent.click(within(toolbar).getByRole("button", { name: "Source" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Comment" }), {
      target: { value: '<p onclick="x()" style="position:fixed">y</p><script>x()</script>' },
    });
    fireEvent.click(within(toolbar).getByRole("button", { name: "Source" }));
    expect(field.innerHTML).toBe("<p>y</p>");
  });
});

describe("RichCommentEditor length meter", () => {
  const meter = () => screen.getByRole("meter");

  it("counts against message_length and warns near the limit", () => {
    const { view } = setup("<p>hello</p>", { text: 14, image: 1000 });
    expect(meter()).toHaveAttribute("aria-valuetext", "12 / 14");
    expect(view.container.querySelector(".rte")).toHaveAttribute("data-level", "near");
  });

  it("turns red and explains when over the limit", () => {
    const { view } = setup("<p>hello world</p>", { text: 10, image: 1000 });
    expect(view.container.querySelector(".rte")).toHaveAttribute("data-level", "over");
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Too long for this server: 18 of 10 characters.",
    );
  });

  it("switches to image_message_length once there is an image", () => {
    setup('<p><img src="data:image/png;base64,AAAA"></p>', { text: 5, image: 2048 });
    expect(screen.getByRole("meter", { name: "Size with images" })).toHaveAttribute(
      "aria-valuetext",
      "0.0 / 2 KB",
    );
  });
});

describe("RichCommentEditor paste", () => {
  const clipboard = (data: Record<string, string>, files: File[] = []) => ({
    clipboardData: { files, getData: (type: string) => data[type] ?? "" },
  });

  it("leaves a paste that lists no text to the webview", () => {
    const { field } = setup();
    expect(fireEvent.paste(field, clipboard({}))).toBe(true);
  });

  it("takes over a paste that lists text", () => {
    const { field } = setup();
    expect(fireEvent.paste(field, clipboard({ "text/plain": "hi" }))).toBe(false);
    expect(execCommand).toHaveBeenCalledWith("insertText", false, "hi");
  });

  it("embeds a pasted image", async () => {
    vi.mocked(prepareImage).mockResolvedValue({
      ok: true,
      dataUrl: "data:image/png;base64,AAAA",
      width: 10,
      height: 10,
      scaled: false,
    });
    const { field } = setup();
    const image = new File(["x"], "shot.png", { type: "image/png" });
    expect(fireEvent.paste(field, clipboard({}, [image]))).toBe(false);
    await waitFor(() => {
      expect(execCommand).toHaveBeenCalledWith(
        "insertHTML",
        false,
        '<img src="data:image/png;base64,AAAA" />',
      );
    });
  });
});
