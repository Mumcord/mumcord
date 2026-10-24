import { fireEvent, render, screen } from "@testing-library/react";
import { beforeAll, describe, expect, it } from "vitest";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { ChannelSearch } from "../channels/ChannelSearch";
import { Composer } from "./Composer";

const LIMITS = { text: 5000, image: 100_000, allowHtml: true };

function renderComposer() {
  const { container } = render(
    <main>
      <ChannelSearch value="" onChange={() => {}} onClear={() => {}} onSubmit={() => {}} />
      <Composer
        target={{ kind: "channel", channelId: 1 }}
        limits={LIMITS}
        label="Message #Lobby"
        blockedReason={null}
      />
    </main>,
  );
  const input = screen.getByRole<HTMLTextAreaElement>("textbox", { name: "Message #Lobby" });
  return { container, input };
}

function type(input: HTMLTextAreaElement, text: string, start: number, end = start) {
  fireEvent.change(input, { target: { value: text } });
  input.focus();
  input.setSelectionRange(start, end);
}

beforeAll(async () => {
  await applyLanguage("en");
});

describe("Composer formatting", () => {
  it("wraps the selection in bold with Ctrl+B and keeps it selected", () => {
    const { input } = renderComposer();
    type(input, "say hi", 4, 6);
    fireEvent.keyDown(input, { key: "b", ctrlKey: true });
    expect(input.value).toBe("say **hi**");
    expect([input.selectionStart, input.selectionEnd]).toEqual([6, 8]);
  });

  it("inserts italic markers with the caret between them from the toolbar", () => {
    const { input } = renderComposer();
    type(input, "a ", 2);
    const button = screen.getByRole("button", { name: "Italic" });
    // The toolbar must not take the focus away from the input.
    expect(fireEvent.mouseDown(button)).toBe(false);
    fireEvent.click(button);
    expect(input.value).toBe("a **");
    expect(document.activeElement).toBe(input);
    expect([input.selectionStart, input.selectionEnd]).toEqual([3, 3]);
  });

  it("inserts a code block with Ctrl+Shift+E", () => {
    const { input } = renderComposer();
    type(input, "", 0);
    fireEvent.keyDown(input, { key: "E", ctrlKey: true, shiftKey: true });
    expect(input.value).toBe("```\n\n```");
    expect(input.selectionStart).toBe(4);
  });

  it("uses Ctrl+K for a link inside the input, for the channel search elsewhere", () => {
    const { input } = renderComposer();
    const search = screen.getByRole("searchbox");
    type(input, "docs", 0, 4);
    fireEvent.keyDown(input, { key: "k", ctrlKey: true });
    expect(input.value).toBe("[docs]()");
    expect(document.activeElement).toBe(input);

    input.blur();
    fireEvent.keyDown(document.body, { key: "k", ctrlKey: true });
    expect(document.activeElement).toBe(search);
  });

  it("offers a labelled toolbar with shortcuts in the tooltips and one Tab stop", async () => {
    const { container } = renderComposer();
    const toolbar = screen.getByRole("toolbar", { name: "Formatting" });
    const buttons = Array.from(toolbar.querySelectorAll("button"));
    expect(buttons.map((b) => b.getAttribute("aria-label"))).toEqual([
      "Bold",
      "Italic",
      "Strikethrough",
      "Inline code",
      "Code block",
      "Link",
    ]);
    expect(buttons[0]?.title).toBe("Bold (Ctrl+B)");
    expect(buttons.filter((b) => b.tabIndex === 0)).toHaveLength(1);
    buttons[0]?.focus();
    fireEvent.keyDown(buttons[0] as HTMLButtonElement, { key: "ArrowLeft" });
    expect(document.activeElement).toBe(buttons[5]);
    expect(await axeViolations(container)).toEqual([]);
  });
});
