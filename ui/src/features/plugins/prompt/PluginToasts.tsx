import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { dismissPluginToast, usePluginToasts, type PluginToast } from "./installToast";
import "./prompt.css";

const TOAST_MS = 6000;

/** Shows the plugin toasts in a corner of the window. Mounted once by PluginsHost. */
export function PluginToasts() {
  const toasts = usePluginToasts();
  return (
    <div className="plugin-toasts" role="status" aria-live="polite">
      {toasts.map((toast) => (
        <ToastItem key={toast.id} toast={toast} />
      ))}
    </div>
  );
}

function ToastItem({ toast }: { toast: PluginToast }) {
  const { t } = useTranslation("pluginBrowse");
  useEffect(() => {
    const timer = window.setTimeout(() => {
      dismissPluginToast(toast.id);
    }, TOAST_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [toast.id]);
  return (
    <div className="plugin-toast">
      <span>{toast.text}</span>
      <button
        type="button"
        className="plugin-toast-dismiss"
        aria-label={t("toast.dismiss")}
        onClick={() => {
          dismissPluginToast(toast.id);
        }}
      >
        ×
      </button>
    </div>
  );
}
