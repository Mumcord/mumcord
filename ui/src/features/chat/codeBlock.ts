/** Larger blocks are not offered for download; a chat message never comes close. */
export const MAX_DOWNLOAD_BYTES = 1024 * 1024;

/** The plain text of a code block, without the highlighting markup. */
export function codeOf(block: HTMLElement): string {
  return block.querySelector("pre")?.textContent ?? "";
}
