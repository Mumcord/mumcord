/**
 * Small pieces the screen share views have in common (issue #163): the path tag and the live
 * sound level.
 */
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { P2pPath } from "../../api";
import { BoltIcon, RelayIcon } from "./icons";
import { litBars } from "./shareFormat";

/** Direct (green) or through the relay (amber), as the chips and the viewer rows show it. */
export function PathTag({ path, chip = false }: { path: P2pPath; chip?: boolean }): ReactNode {
  const { t } = useTranslation("screenviewer");
  const direct = path === "direct";
  return (
    <span
      className={`share-path share-path--${direct ? "direct" : "relay"}${chip ? " share-chip" : ""}`}
    >
      {direct ? <BoltIcon /> : <RelayIcon />}
      {t(direct ? "pathDirect" : "pathRelay")}
    </span>
  );
}

/**
 * "Priya's sound" with five bars that follow the shared sound, so a viewer sees there is sound even
 * when they turned it down.
 */
export function SoundLevel({ label, rms }: { label: string; rms: number | null }): ReactNode {
  const lit = litBars(rms);
  return (
    <span className="share-sound">
      <span className="share-sound__bars" aria-hidden="true">
        {[1, 2, 3, 4, 5].map((bar) => (
          <span key={bar} className="share-sound__bar" data-lit={bar <= lit || undefined} />
        ))}
      </span>
      {label}
    </span>
  );
}
