import { useCallback, useEffect, useState } from "react";
import { getApi, type CrashReport } from "../../../api";

/** The saved crash reports and what can be done with them (#568). */
export function useCrashReports() {
  const [reports, setReports] = useState<CrashReport[] | null>(null);

  const reload = useCallback(async () => {
    try {
      setReports(await getApi().crashList());
    } catch (error: unknown) {
      console.error("Listing crash reports failed", error);
      setReports([]);
    }
  }, []);

  useEffect(() => {
    let active = true;
    getApi()
      .crashList()
      .then(
        (list) => {
          if (active) setReports(list);
        },
        (error: unknown) => {
          console.error("Listing crash reports failed", error);
          if (active) setReports([]);
        },
      );
    return () => {
      active = false;
    };
  }, []);

  return {
    reports,
    reload,
    read: (id: string) => getApi().crashRead(id),
    remove: async (id: string) => {
      await getApi().crashDelete(id);
      await reload();
    },
    removeAll: async () => {
      await getApi().crashDeleteAll();
      await reload();
    },
    exportTo: (ids: string[], path: string) => getApi().crashExport(ids, path),
    openFolder: () => getApi().crashOpenFolder(),
  };
}

/** "Nov 6, 2026 · 09:41" in the UI language. */
export function formatWhen(report: CrashReport, language: string): string {
  const date = new Date((report.created ?? 0) * 1000);
  const day = new Intl.DateTimeFormat(language, { dateStyle: "medium" }).format(date);
  const time = new Intl.DateTimeFormat(language, { timeStyle: "short" }).format(date);
  return `${day} · ${time}`;
}

/** The locale key of a report's kind chip. */
export function kindKey(report: CrashReport): "kindNative" | "kindPanic" | "kindUnexpected" {
  switch (report.kind) {
    case "panic":
      return "kindPanic";
    case "signal":
      return "kindNative";
    case "unexpected":
      return "kindUnexpected";
  }
}
