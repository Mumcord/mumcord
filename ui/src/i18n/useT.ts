import i18next, { type TFunction } from "i18next";
import { useSyncExternalStore } from "react";

const subscribe = (onChange: () => void): (() => void) => {
  i18next.on("languageChanged", onChange);
  return () => {
    i18next.off("languageChanged", onChange);
  };
};
const language = (): string => i18next.language;

const fixed = new Map<string, TFunction>();

/**
 * The translate function of one namespace for components that are mounted by the hundreds (rows
 * of the channel tree, member list and chat). `useTranslation` copies the whole i18n instance on
 * every mount, which made switching to a big server hold the thread for several hundred
 * milliseconds on a slow CPU (AC-2.17). All bundles of a language are loaded before it is
 * active, so nothing here needs to wait for a namespace.
 */
export function useT(namespace: string): TFunction {
  const lng = useSyncExternalStore(subscribe, language, language);
  const key = `${lng}|${namespace}`;
  let t = fixed.get(key);
  if (t === undefined) {
    t = i18next.getFixedT(lng, namespace);
    fixed.set(key, t);
  }
  return t;
}
