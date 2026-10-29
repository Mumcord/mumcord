/**
 * Plain text of the user's selection in the chat (#114). The browser's own `toString()` runs
 * all lines together and includes text the user never saw: screen-reader labels, the hidden
 * hover time of grouped messages, avatar initials and the labels of the action buttons.
 */

/** UI parts that are not message text, even when a selection spans them. */
const HIDDEN =
  '.visually-hidden, [aria-hidden="true"], .chat-message__hover-time, .chat-message__actions, .chat-code__actions';

const BLOCKS = new Set([
  "ADDRESS",
  "ARTICLE",
  "ASIDE",
  "BLOCKQUOTE",
  "DD",
  "DIV",
  "DL",
  "DT",
  "FIGCAPTION",
  "FIGURE",
  "FOOTER",
  "H1",
  "H2",
  "H3",
  "H4",
  "H5",
  "H6",
  "HEADER",
  "HR",
  "LI",
  "OL",
  "P",
  "PRE",
  "SECTION",
  "TABLE",
  "TR",
  "UL",
]);

/** Stands for a line break between blocks; runs of them collapse into one. */
const BREAK = "";

function collect(node: Node, inPre: boolean, out: string[]): void {
  if (node instanceof Text) {
    out.push(inPre ? node.data : node.data.replace(/\s+/g, " "));
    return;
  }
  if (!(node instanceof Element) && !(node instanceof DocumentFragment)) return;
  if (node instanceof Element) {
    if (node.matches(HIDDEN)) return;
    if (node.tagName === "BR") {
      out.push("\n");
      return;
    }
  }
  const block = node instanceof Element && BLOCKS.has(node.tagName);
  const pre = inPre || (node instanceof Element && node.tagName === "PRE");
  if (block) out.push(BREAK);
  for (const child of node.childNodes) collect(child, pre, out);
  if (block) out.push(BREAK);
}

/** The selected text inside `root`, or "" when nothing there is selected. */
export function selectedText(selection: Selection | null, root: Node | null): string {
  if (!root || !selection || selection.rangeCount === 0 || selection.isCollapsed) return "";
  const out: string[] = [];
  for (let i = 0; i < selection.rangeCount; i++) {
    const range = selection.getRangeAt(i);
    if (!range.intersectsNode(root)) continue;
    const common = range.commonAncestorContainer;
    const around = common instanceof Element ? common : common.parentElement;
    if (around?.closest(HIDDEN)) continue;
    const clipped = range.cloneRange();
    // Only the part inside the chat: a selection may run on into the sidebar.
    if (!root.contains(range.startContainer)) clipped.setStart(root, 0);
    if (!root.contains(range.endContainer)) clipped.setEnd(root, root.childNodes.length);
    // The clone keeps the classes of the partly selected ancestors, so hidden parts stay known.
    collect(clipped.cloneContents(), around?.closest("pre") != null, out);
    out.push(BREAK);
  }
  return out
    .join("")
    .replace(new RegExp(` *(?:${BREAK} *)+`, "g"), BREAK)
    .replace(new RegExp(`^${BREAK}|${BREAK}$`, "g"), "")
    .replaceAll(BREAK, "\n");
}
