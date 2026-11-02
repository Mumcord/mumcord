import { createContext, useContext } from "react";
import type { PluginActionValue } from "../../../api";

export interface RenderContext {
  plugin: string;
  /** Sends a `ui-action` for the slot being rendered. */
  send: (action: string, value: PluginActionValue) => void;
  reducedMotion: boolean;
}

export const RenderCtx = createContext<RenderContext | null>(null);

export function useRender(): RenderContext {
  const ctx = useContext(RenderCtx);
  if (ctx === null) throw new Error("plugin components must render inside PluginTree");
  return ctx;
}
