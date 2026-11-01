/** The clock a media view uses to show how far it is (issue #163); recomputed once a second. */
import { useEffect, useState } from "react";

export function useTick(ms = 1000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => {
      setNow(Date.now());
    }, ms);
    return () => {
      window.clearInterval(timer);
    };
  }, [ms]);
  return now;
}
