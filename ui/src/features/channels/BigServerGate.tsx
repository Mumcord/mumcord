import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

interface BigServerGateProps {
  /** Whether the server is big enough that drawing its lists takes noticeable time. */
  big: boolean;
  /** Class of the "Syncing big server…" line, matching the panel it stands in for. */
  className: string;
  children: ReactNode;
}

/**
 * Shows "Syncing big server…" instead of a frozen window while a big server's list is first
 * drawn (FR-2.23, FR-2.24): the placeholder is on screen for a frame before the list (which draws
 * itself step by step, see `useDrawSteps`) starts. Small servers show their list right away. Key
 * the gate by server so every newly shown server starts with the placeholder.
 */
export function BigServerGate({ big, className, children }: BigServerGateProps) {
  const { t } = useTranslation("channels");
  const [ready, setReady] = useState(!big);
  useEffect(() => {
    if (ready) return;
    // After a frame, so the placeholder is on screen before the list is rendered.
    const frame = requestAnimationFrame(() => {
      setReady(true);
    });
    return () => {
      cancelAnimationFrame(frame);
    };
  }, [ready]);
  return ready ? (
    children
  ) : (
    <p className={className} role="status">
      {t("empty.syncingBig")}
    </p>
  );
}
