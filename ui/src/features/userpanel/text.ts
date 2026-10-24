/** Mumble comments are HTML; the editor works on plain text. */
export function textToHtml(text: string): string {
  const escaped = text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  return escaped.trim() === "" ? "" : escaped.replace(/\r?\n/g, "<br>");
}

export function htmlToText(html: string): string {
  const doc = new DOMParser().parseFromString(html.replace(/<br\s*\/?>/gi, "\n"), "text/html");
  return doc.body.textContent;
}
