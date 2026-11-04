import { useEffect, useRef } from "react";
import { announce } from "../../../a11y";

/**
 * Reads a notice out politely once when it appears or its text changes, and not again on each
 * poll of the share's quality (FR-7.38, FR-7.57, FR-7.58).
 */
export function useAnnounceChange(message: string | null): void {
  const last = useRef<string | null>(null);
  useEffect(() => {
    if (message !== null && message !== last.current) announce(message, "polite");
    last.current = message;
  }, [message]);
}
