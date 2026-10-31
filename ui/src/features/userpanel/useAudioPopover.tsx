import { useCallback, useEffect, useState, type KeyboardEvent, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { getApi } from "../../api";
import { AudioPopover, type AudioAnchor, type AudioKind } from "./AudioPopover";

/** How long the "device unplugged" notice stays on screen. */
export const NOTICE_MS = 6000;

interface Open {
  kind: AudioKind;
  anchor: AudioAnchor;
}

export interface AudioTriggers {
  onContextMenu: (event: MouseEvent<HTMLElement>) => void;
  onKeyDown: (event: KeyboardEvent<HTMLElement>) => void;
}

function belowOf(element: HTMLElement): AudioAnchor {
  const box = element.getBoundingClientRect();
  return { x: box.left, y: box.bottom, top: box.top, opener: element };
}

/**
 * The audio popovers of the voice bar's mute and deafen buttons, plus the short notice shown when
 * a chosen device disappeared and the system default took over. `node` goes into the voice bar;
 * `triggers` and `open` go onto the buttons.
 */
export function useAudioPopover(onVoiceSettings: () => void) {
  const { t } = useTranslation("userpanel");
  const [open, setOpen] = useState<Open | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const close = useCallback(() => {
    setOpen(null);
  }, []);

  useEffect(
    () =>
      getApi().on("audioDeviceFallback", ({ input }) => {
        const text = input ? t("audio.fallbackInput") : t("audio.fallbackOutput");
        setNotice(text);
        announce(text);
      }),
    [t],
  );

  useEffect(() => {
    if (notice === null) return;
    const timer = setTimeout(() => {
      setNotice(null);
    }, NOTICE_MS);
    return () => {
      clearTimeout(timer);
    };
  }, [notice]);

  const triggers = (kind: AudioKind): AudioTriggers => ({
    onContextMenu: (event) => {
      event.preventDefault();
      setOpen({
        kind,
        anchor: {
          x: event.clientX,
          y: event.clientY,
          top: event.clientY,
          opener: event.currentTarget,
        },
      });
    },
    onKeyDown: (event) => {
      if (event.key !== "ContextMenu" && !(event.key === "F10" && event.shiftKey)) return;
      event.preventDefault();
      setOpen({ kind, anchor: belowOf(event.currentTarget) });
    },
  });

  const node = (
    <>
      {open ? (
        <AudioPopover
          kind={open.kind}
          anchor={open.anchor}
          onClose={close}
          onVoiceSettings={onVoiceSettings}
        />
      ) : null}
      {notice !== null ? (
        <p className="userpanel-notice" role="status">
          {notice}
        </p>
      ) : null}
    </>
  );

  return {
    node,
    triggers,
    isOpen: (kind: AudioKind) => open?.kind === kind,
    openFrom: (kind: AudioKind, opener: HTMLElement) => {
      setOpen({ kind, anchor: belowOf(opener) });
    },
  };
}
