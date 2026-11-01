import { Suspense, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

/**
 * Suspense boundary for a part of the app that is loaded as its own chunk (issue #323), e.g.
 * the settings or the setup wizard. The chunk comes from the app's own files and arrives within
 * a few milliseconds, so the fallback stays empty instead of flashing a "Loading…" text; it is
 * only marked busy for assistive technology.
 */
export function LazyBoundary({ children }: { children: ReactNode }) {
  return <Suspense fallback={<LazyFallback />}>{children}</Suspense>;
}

function LazyFallback() {
  const { t } = useTranslation("common");
  return (
    <div aria-busy="true">
      <span className="visually-hidden">{t("loading")}</span>
    </div>
  );
}
