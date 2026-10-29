import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type CacheKind, type CacheSizes } from "../../../api";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";
import { errorText } from "../errors";
import { formatSize } from "../formatSize";

type Status = { kind: "success" | "error"; text: string } | null;

const KINDS: readonly CacheKind[] = ["previews", "webEngine", "temp"];
const KIB = 1024;

function sizeOf(sizes: CacheSizes, kind: CacheKind): number {
  switch (kind) {
    case "previews":
      return sizes.previewsKib;
    case "webEngine":
      return sizes.webKib;
    case "temp":
      return sizes.tempKib;
  }
}

/**
 * Size of the caches per category and the "Clear cache" action. Only caches are deleted:
 * history, settings, servers, certificates, friends and notes are never touched.
 */
export function CacheGroup() {
  const { t } = useTranslation("settings");
  const [sizes, setSizes] = useState<CacheSizes | null>(null);
  const [selected, setSelected] = useState<ReadonlySet<CacheKind>>(new Set(KINDS));
  const [confirming, setConfirming] = useState(false);
  const [status, setStatus] = useState<Status>(null);

  const measure = useCallback(async () => {
    try {
      setSizes(await getApi().cacheSizes());
    } catch (error: unknown) {
      console.error("Measuring the caches failed", error);
      setStatus({ kind: "error", text: errorText(t, error) });
    }
  }, [t]);

  useEffect(() => {
    let active = true;
    getApi()
      .cacheSizes()
      .then((measured) => {
        if (active) setSizes(measured);
      })
      .catch((error: unknown) => {
        console.error("Measuring the caches failed", error);
        if (active) setStatus({ kind: "error", text: errorText(t, error) });
      });
    return () => {
      active = false;
    };
  }, [t]);

  const toggle = (kind: CacheKind, on: boolean) => {
    setSelected((current) => {
      const next = new Set(current);
      if (on) next.add(kind);
      else next.delete(kind);
      return next;
    });
  };

  const clear = async () => {
    setConfirming(false);
    setStatus(null);
    try {
      const result = await getApi().cacheClear(KINDS.filter((kind) => selected.has(kind)));
      const cleared = t("cache.cleared", { size: formatSize(result.freedKib * KIB) });
      setStatus({
        kind: "success",
        text: result.webPending ? `${cleared} ${t("cache.webPending")}` : cleared,
      });
      await measure();
    } catch (error: unknown) {
      console.error("Clearing the caches failed", error);
      setStatus({ kind: "error", text: errorText(t, error) });
    }
  };

  const sizeText = (kib: number | null) =>
    kib === null ? t("cache.calculating") : formatSize(kib * KIB);
  const total = sizes === null ? null : KINDS.reduce((sum, kind) => sum + sizeOf(sizes, kind), 0);

  return (
    <Group title={t("cache.title")}>
      <p className="settings-hint">{t("cache.hint")}</p>
      {KINDS.map((kind) => (
        <Switch
          key={kind}
          label={`${t(`cache.kind.${kind}`)} (${sizeText(sizes === null ? null : sizeOf(sizes, kind))})`}
          checked={selected.has(kind)}
          onChange={(on) => {
            toggle(kind, on);
          }}
        />
      ))}
      <p className="settings-value">{t("cache.total", { size: sizeText(total) })}</p>
      <button
        type="button"
        className="settings-button"
        disabled={selected.size === 0}
        onClick={() => {
          setStatus(null);
          setConfirming(true);
        }}
      >
        {t("cache.clear")}
      </button>
      <p role="status" className={status?.kind === "error" ? "settings-error" : "settings-success"}>
        {status?.text}
      </p>
      {confirming && (
        <ConfirmDialog
          title={t("cache.confirmTitle")}
          message={t("cache.confirm")}
          confirmLabel={t("cache.clear")}
          onConfirm={() => void clear()}
          onCancel={() => {
            setConfirming(false);
          }}
        />
      )}
    </Group>
  );
}
