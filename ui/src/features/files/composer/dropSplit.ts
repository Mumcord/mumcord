/** Splits dropped paths into images the chat can send inline and everything else. */
const INLINE_IMAGE = /\.(png|jpe?g|gif|webp)$/i;

export function splitDropped(paths: readonly string[]): { images: string[]; others: string[] } {
  const images: string[] = [];
  const others: string[] = [];
  for (const path of paths) (INLINE_IMAGE.test(path) ? images : others).push(path);
  return { images, others };
}
