/**
 * Thumbnails of the images waiting to be sent: size, a note when scaled down, a spinner while
 * an image is still being processed, and a remove button for each.
 */
import { useTranslation } from "react-i18next";
import type { Attachment } from "./useComposer";

function formatSize(dataUrlLength: number): string {
  // A base64 data URL carries about 3 bytes per 4 characters.
  const bytes = Math.round((dataUrlLength * 3) / 4);
  return bytes >= 1024 * 1024
    ? `${(bytes / (1024 * 1024)).toFixed(1)} MB`
    : `${String(Math.max(1, Math.round(bytes / 1024)))} KB`;
}

export function AttachmentTray({
  items,
  onRemove,
}: {
  items: readonly Attachment[];
  onRemove: (id: number) => void;
}) {
  const { t } = useTranslation("chat");
  if (items.length === 0) return null;
  return (
    <ul className="chat-tray" aria-label={t("tray.label")}>
      {items.map((item) => (
        <li key={item.id} className="chat-tray__item" aria-busy={item.status === "processing"}>
          {item.status === "processing" ? (
            <span className="chat-tray__spinner" role="status" aria-label={t("tray.processing")} />
          ) : (
            <img src={item.dataUrl} alt={t("composer.imagePreview", { name: item.name })} />
          )}
          <span className="chat-tray__info">
            <span className="chat-tray__name">{item.name}</span>
            {item.status === "ready" ? (
              <span>
                {item.width}×{item.height}, {formatSize(item.dataUrl.length)}
              </span>
            ) : null}
            {item.scaled ? (
              <span className="chat-tray__note">
                {t("tray.scaled", { width: item.width, height: item.height })}
              </span>
            ) : null}
          </span>
          <button
            type="button"
            className="chat-tray__remove"
            aria-label={t("composer.removeImage", { name: item.name })}
            data-tooltip={t("composer.removeImage", { name: item.name })}
            onClick={() => {
              onRemove(item.id);
            }}
          >
            ×
          </button>
        </li>
      ))}
    </ul>
  );
}
