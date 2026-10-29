import { useEffect, useState } from "react";
import { readRootLook, type RootLook } from "./inspectorView";

/** The main window's root look, updated whenever its theme, hints or language change. */
export function useRootLook(): RootLook {
  const [look, setLook] = useState(() => readRootLook(document.documentElement));
  useEffect(() => {
    const root = document.documentElement;
    const observer = new MutationObserver(() => {
      setLook(readRootLook(root));
    });
    observer.observe(root, { attributes: true });
    return () => {
      observer.disconnect();
    };
  }, []);
  return look;
}
