import { useEffect, useState } from "react";
import type { PluginComponent } from "../../../api";
import { usePluginAsset } from "./assets";
import { useRender } from "./context";

/** Highest sprite frame rate (FR-6.41); the host refuses more, this guards the renderer too. */
export const MAX_FPS = 12;

/**
 * A horizontal strip of frames played in a loop; with reduced motion only the still frame
 * shows (§5.8).
 */
export function PluginSprite(props: Extract<PluginComponent, { type: "sprite" }>) {
  const { asset, alt, frameWidth, frameHeight, frames, fps, stillFrame } = props;
  const { plugin, reducedMotion } = useRender();
  const url = usePluginAsset(plugin, asset);
  const animate = !reducedMotion && frames > 1 && fps > 0;
  const [tick, setTick] = useState(0);

  useEffect(() => {
    if (!animate) return;
    const timer = setInterval(
      () => {
        setTick((t) => t + 1);
      },
      1000 / Math.min(fps, MAX_FPS),
    );
    return () => {
      clearInterval(timer);
    };
  }, [animate, fps]);

  if (url === null) return alt === "" ? null : <span className="pr-alt">{alt}</span>;
  const frame = animate ? tick % frames : Math.min(stillFrame, Math.max(frames - 1, 0));
  return (
    <span
      className="pr-sprite"
      role={alt === "" ? undefined : "img"}
      aria-label={alt === "" ? undefined : alt}
      aria-hidden={alt === "" ? true : undefined}
      data-frame={frame}
      style={{
        width: frameWidth,
        height: frameHeight,
        backgroundImage: url === undefined ? undefined : `url("${url}")`,
        backgroundPosition: `-${String(frame * frameWidth)}px 0`,
      }}
    />
  );
}
