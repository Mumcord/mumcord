/**
 * The pop-out parts of the media views (issue #164): "Pop out" in the main window's overlay,
 * always on top and "Pop back in" in the pop-out's overlay, and the placeholder the main window
 * shows while the view plays in its own window.
 */
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { MediaPopoutKind } from "../../api";
import { MediaIcon } from "./icons";
import { popIn, popOut, setPopoutOnTop, useInPopout, usePopouts } from "./popoutStore";

/**
 * The window buttons of a media overlay: "Pop out" in the main window; always on top and
 * "Pop back in" in the pop-out.
 */
export function PopoutButtons({
  kind,
  serverId,
  session,
  title,
}: {
  kind: MediaPopoutKind;
  serverId: number;
  /** The sharer; `null` for the watch party. */
  session: number | null;
  /** The window title. */
  title: string;
}): ReactNode {
  const { t } = useTranslation("common");
  const inPopout = useInPopout();
  const onTop = usePopouts()[kind].onTop;
  if (!inPopout) {
    return (
      <button
        type="button"
        className="button media-frame__icon"
        aria-label={t("media.popOut")}
        data-tooltip={t("media.popOut")}
        onClick={() => {
          popOut(kind, { serverId, session }, title, t("media.popOutFailed"));
        }}
      >
        <MediaIcon name="open" />
      </button>
    );
  }
  return (
    <>
      <button
        type="button"
        className="button media-frame__icon"
        aria-label={t("media.onTop")}
        data-tooltip={t("media.onTop")}
        aria-pressed={onTop}
        onClick={() => {
          setPopoutOnTop(kind, !onTop, t("media.onTopFailed"));
        }}
      >
        <MediaIcon name="pin" />
      </button>
      <button
        type="button"
        className="button media-frame__icon"
        aria-label={t("media.popIn")}
        data-tooltip={t("media.popIn")}
        aria-keyshortcuts="Control+W"
        onClick={() => {
          popIn(kind, t("media.popInFailed"));
        }}
      >
        <MediaIcon name="popIn" />
      </button>
    </>
  );
}

/** What the main window shows while the view plays in its own window. */
export function PoppedOut({ kind, title }: { kind: MediaPopoutKind; title: string }): ReactNode {
  const { t } = useTranslation("common");
  return (
    <section className="media-popped-out" aria-label={title}>
      <MediaIcon name="open" size={24} />
      <p className="media-popped-out__status" role="status">
        {t("media.poppedOut")}
      </p>
      <p className="media-popped-out__title">{title}</p>
      <button
        type="button"
        className="button"
        onClick={() => {
          popIn(kind, t("media.popInFailed"));
        }}
      >
        {t("media.bringBack")}
      </button>
    </section>
  );
}
