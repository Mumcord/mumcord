import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { RichCommentEditor } from "./RichCommentEditor";

function setup() {
  render(
    <>
      <span id="label">Comment</span>
      <RichCommentEditor
        labelId="label"
        initialHtml=""
        loadKey={0}
        disabled={false}
        onChange={vi.fn()}
      />
    </>,
  );
  return screen.getByRole("textbox", { name: "Comment" });
}

describe("RichCommentEditor paste", () => {
  it("leaves a paste that lists no text to the webview", () => {
    const field = setup();
    const notPrevented = fireEvent.paste(field, { clipboardData: { getData: () => "" } });
    expect(notPrevented).toBe(true);
  });

  it("takes over a paste that lists text", () => {
    const field = setup();
    // eslint-disable-next-line @typescript-eslint/no-deprecated
    document.execCommand = vi.fn(() => true);
    const notPrevented = fireEvent.paste(field, {
      clipboardData: { getData: (type: string) => (type === "text/plain" ? "hi" : "") },
    });
    expect(notPrevented).toBe(false);
  });
});
