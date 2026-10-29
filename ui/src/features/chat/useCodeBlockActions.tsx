import { useLayoutEffect, useState, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";
import { CodeBlockActions } from "./CodeBlockActions";
import { CODE_BLOCK_CLASS } from "./highlight";

/**
 * The buttons of every code block in HTML rendered into `ref`. The blocks are found after each
 * render of `html`, before paint, so the buttons appear together with the message.
 */
export function useCodeBlockActions(
  ref: RefObject<HTMLElement | null>,
  html: string,
  tabIndex: number,
): ReactNode {
  const [blocks, setBlocks] = useState<HTMLElement[]>([]);
  useLayoutEffect(() => {
    setBlocks([...(ref.current?.querySelectorAll<HTMLElement>(`.${CODE_BLOCK_CLASS}`) ?? [])]);
  }, [ref, html]);
  return blocks.map((block, index) =>
    createPortal(<CodeBlockActions block={block} tabIndex={tabIndex} />, block, String(index)),
  );
}
