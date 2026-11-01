import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { PluginComponent } from "../../../api";
import { formatRemaining } from "./helpers";

/** Counts down to `until` (Unix seconds) itself, so the plugin needs no 1 s timer. */
export function PluginCountdown({ label, until }: Extract<PluginComponent, { type: "countdown" }>) {
  const { t } = useTranslation("pluginSlots");
  const [now, setNow] = useState(() => Date.now() / 1000);
  const remaining = (until ?? 0) - now;
  const done = remaining <= 0;

  useEffect(() => {
    if (done) return;
    const timer = setInterval(() => {
      setNow(Date.now() / 1000);
    }, 1000);
    return () => {
      clearInterval(timer);
    };
  }, [done]);

  return (
    <div className="pr-countdown">
      <span className="pr-countdown__label">{label}</span>
      {/* Only the finished state is announced; a ticking live region would talk every second. */}
      <span className="pr-countdown__time" role="timer" aria-live={done ? "polite" : "off"}>
        {done ? t("countdown.done") : formatRemaining(remaining)}
      </span>
    </div>
  );
}
