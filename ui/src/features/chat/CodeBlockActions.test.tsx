import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import {
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
  type MockInstance,
} from "vitest";
import { createMockApi, setApi, type ChatMessage, type MockApi } from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { CodeBlockActions } from "./CodeBlockActions";
import { MAX_DOWNLOAD_BYTES } from "./codeBlock";
import { extensionFor, loadHighlighter } from "./highlight";
import { markdownToHtml } from "./markdown";
import { MessageList } from "./MessageList";

const SQL = "SELECT name, count(*) FROM users\nWHERE active = 1\nGROUP BY name;";

function message(body: string): ChatMessage {
  return {
    id: "1",
    conversation: { kind: "channel", channelId: 1 },
    senderName: "Ada",
    senderCertHash: null,
    senderSession: null,
    own: false,
    sentAt: new Date().toISOString(),
    body,
    dismissedPreviews: [],
    syncedFrom: null,
    file: null,
  };
}

function renderMessage(body: string, syncedFrom: string | null = null) {
  return render(
    <main>
      <MessageList
        log={{
          messages: [{ ...message(body), syncedFrom }],
          loaded: true,
          loading: false,
          hasMore: false,
          loadFailed: false,
          syncing: false,
          syncResult: null,
          syncFailed: false,
        }}
        label="Messages in #dev"
        onLoadOlder={() => undefined}
      />
    </main>,
  );
}

let api: MockApi;
let writeText: MockInstance<MockApi["copyText"]>;

beforeAll(async () => {
  await applyLanguage("en");
  await loadHighlighter();
});

beforeEach(() => {
  api = createMockApi();
  setApi(api);
  writeText = vi.spyOn(api, "copyText").mockResolvedValue(null);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("code block buttons", () => {
  it("shows copy and download buttons on a fenced block", () => {
    const { container } = renderMessage(markdownToHtml("```\n" + SQL + "\n```"));
    const block = container.querySelector(".chat-code");
    expect(block?.querySelector("pre")).not.toBeNull();
    const buttons = within(block as HTMLElement);
    expect(buttons.getByRole("button", { name: "Copy code" })).toBeTruthy();
    expect(buttons.getByRole("button", { name: "Download code" })).toBeTruthy();
  });

  it("gives inline code no buttons", () => {
    renderMessage(markdownToHtml("run `const x = await fetch(url);` now"));
    expect(screen.queryByRole("button", { name: "Copy code" })).toBeNull();
  });

  it("copies the plain text, not the highlighted HTML, and says so", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const { container } = renderMessage(markdownToHtml("```\n" + SQL + "\n```"));
    expect(container.querySelector(".chat-code .hljs-keyword")).not.toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Copy code" }));
    await waitFor(() => {
      expect(screen.getByRole("status").textContent).toBe("Copied");
    });
    expect(writeText).toHaveBeenCalledWith(SQL + "\n");
    await act(() => vi.advanceTimersByTimeAsync(2500));
    expect(screen.getByRole("status").textContent).toBe("");
  });

  it("reports a failed copy", async () => {
    writeText.mockRejectedValue(new Error("denied"));
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    renderMessage(markdownToHtml("```\n" + SQL + "\n```"));
    fireEvent.click(screen.getByRole("button", { name: "Copy code" }));
    await waitFor(() => {
      expect(screen.getByRole("status").textContent).toBe("Could not copy the code.");
    });
  });

  it("downloads the block as snippet.<ext> of the detected language", async () => {
    const save = vi.spyOn(api, "saveText").mockResolvedValue(true);
    renderMessage(markdownToHtml("```\n" + SQL + "\n```"));
    fireEvent.click(screen.getByRole("button", { name: "Download code" }));
    await waitFor(() => {
      expect(screen.getByRole("status").textContent).toBe("Saved");
    });
    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({
        title: "Download code",
        defaultName: "snippet.sql",
        filterName: "Code file",
        extensions: ["sql"],
      }),
      SQL + "\n",
    );
  });

  it("falls back to snippet.txt when the language is unknown", async () => {
    const save = vi.spyOn(api, "saveText").mockResolvedValue(false);
    renderMessage(markdownToHtml("```\nhello there\n```"));
    fireEvent.click(screen.getByRole("button", { name: "Download code" }));
    await waitFor(() => {
      expect(save).toHaveBeenCalled();
    });
    expect(save.mock.calls[0]?.[0].defaultName).toBe("snippet.txt");
    expect(screen.getByRole("status").textContent).toBe("");
  });

  it("refuses to download a block above the size limit", async () => {
    // Built by hand: highlighting a megabyte of text would only slow the test down.
    const block = document.createElement("div");
    const pre = document.createElement("pre");
    pre.textContent = "x".repeat(MAX_DOWNLOAD_BYTES + 1);
    block.append(pre);
    const save = vi.spyOn(api, "saveText");
    render(<CodeBlockActions block={block} tabIndex={0} />);
    fireEvent.click(screen.getByRole("button", { name: "Download code" }));
    await waitFor(() => {
      expect(screen.getByRole("status").textContent).toBe("The code is too large to download.");
    });
    expect(save).not.toHaveBeenCalled();
  });

  it("reports a failed save", async () => {
    vi.spyOn(api, "saveText").mockRejectedValue(new Error("disk full"));
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    renderMessage(markdownToHtml("```\nhello\n```"));
    fireEvent.click(screen.getByRole("button", { name: "Download code" }));
    await waitFor(() => {
      expect(screen.getByRole("status").textContent).toBe("Could not save the code.");
    });
  });

  it("is reachable with the keyboard and has no accessibility violations", async () => {
    const { container } = renderMessage(markdownToHtml("```\n" + SQL + "\n```"));
    const copy = screen.getByRole("button", { name: "Copy code" });
    copy.focus();
    expect(document.activeElement).toBe(copy);
    expect(await axeViolations(container)).toEqual([]);
  });
});

describe("language label (issue #112)", () => {
  it("shows the named language in the block's corner", () => {
    const { container } = renderMessage(markdownToHtml("```sql\n" + SQL + "\n```"));
    expect(container.querySelector(".chat-code__label")?.textContent).toBe("SQL");
  });

  it("shows the label on a message that arrived through history sync", () => {
    // The stored body as the engine hands it over after a sync or a restart (issue #122).
    const { container } = renderMessage(
      '<pre data-mumcord-lang="sql"><code>SELECT 1;\n</code></pre>',
      "Bob",
    );
    expect(container.querySelector(".chat-code__label")?.textContent).toBe("SQL");
    expect(container.querySelector(".chat-code .hljs-keyword")?.textContent).toBe("SELECT");
  });

  it("translates the plain text label", () => {
    const { container } = renderMessage(markdownToHtml("```plaintext\nhello\n```"));
    expect(container.querySelector(".chat-code__label")?.textContent).toBe("Plain text");
  });

  it("shows no label for a block whose language was only detected", () => {
    const { container } = renderMessage(markdownToHtml("```\n" + SQL + "\n```"));
    expect(container.querySelector(".chat-code__label")).toBeNull();
  });

  it("names the download after the fence language", async () => {
    const save = vi.spyOn(api, "saveText").mockResolvedValue(false);
    renderMessage(markdownToHtml("```py\nprint('hi')\n```"));
    fireEvent.click(screen.getByRole("button", { name: "Download code" }));
    await waitFor(() => {
      expect(save).toHaveBeenCalled();
    });
    expect(save.mock.calls[0]?.[0].defaultName).toBe("snippet.py");
  });

  it("highlights a lazily loaded language once its grammar arrives", async () => {
    const { container } = renderMessage(markdownToHtml("```lua\nlocal x = nil\n```"));
    expect(container.querySelector(".chat-code__label")?.textContent).toBe("Lua");
    await waitFor(() => {
      expect(container.querySelector(".chat-code .hljs-keyword")).not.toBeNull();
    });
  });
});

describe("extensionFor", () => {
  it("maps known languages and falls back to txt", () => {
    expect(extensionFor("rust")).toBe("rs");
    expect(extensionFor("python")).toBe("py");
    expect(extensionFor("cmd")).toBe("bat");
    expect(extensionFor("hex")).toBe("hex");
    expect(extensionFor("klingon")).toBe("txt");
    expect(extensionFor(undefined)).toBe("txt");
  });
});
