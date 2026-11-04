/**
 * The live bar's Quality menu (FR-7.39, mockup 02): Auto or a fixed step, and what to keep when
 * the share has to step down. A change reaches the running share within a second, without
 * restarting it. The share starts from the Settings defaults, so the menu starts there too.
 */
import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../../a11y";
import { getApi, type QualityChoice, type ScreenFps } from "../../../api";
import { ChevronDownIcon } from "../icons";
import { useDismiss } from "../useDismiss";
import { CheckIcon, GaugeIcon } from "./qualityIcons";
import {
  OPTIMIZE_CHOICES,
  QUALITY_PRESETS,
  checkedRow,
  chooseRow,
  type QualityRow,
} from "./qualityModel";
import "./quality.css";

const ITEM = '[role="menuitemradio"]';

export function QualityMenu({ serverId }: { serverId: number }): ReactNode {
  const { t } = useTranslation("screenshare");
  const [open, setOpen] = useState(false);
  const [choice, setChoice] = useState<QualityChoice | null>(null);
  const [settingsFps, setSettingsFps] = useState<ScreenFps>("30");
  const button = useRef<HTMLButtonElement>(null);
  const popover = useRef<HTMLDivElement>(null);
  const menuId = useId();
  const qualityId = useId();
  const optimizeId = useId();
  // Escape (and a click outside) closes; focus goes back to the button unless the user moved it.
  const close = useCallback(() => {
    const focusInside = popover.current?.contains(document.activeElement) === true;
    setOpen(false);
    if (focusInside) button.current?.focus();
  }, []);
  const dismissRefs = useMemo(() => [button, popover], []);
  useDismiss(dismissRefs, open, close);

  useEffect(() => {
    let current = true;
    getApi()
      .settingsGet()
      .then((settings) => {
        if (!current) return;
        setSettingsFps(settings.screenFps);
        setChoice({
          resolution: settings.screenResolution,
          fps: settings.screenFps,
          optimize: settings.screenOptimize,
          uploadLimit: settings.screenUploadLimit,
        });
      })
      .catch((error: unknown) => {
        console.error("reading the screen sharing settings failed", error);
      });
    return () => {
      current = false;
    };
  }, []);

  // Opening moves focus to the checked row, as in any menu of choices.
  useEffect(() => {
    if (!open) return;
    const items = [...(popover.current?.querySelectorAll<HTMLElement>(ITEM) ?? [])];
    (items.find((item) => item.getAttribute("aria-checked") === "true") ?? items[0])?.focus();
  }, [open]);

  const closeToButton = () => {
    setOpen(false);
    button.current?.focus();
  };

  const apply = (next: QualityChoice) => {
    if (choice === null) return;
    const before = choice;
    setChoice(next);
    closeToButton();
    getApi()
      .screenShareSetQuality(serverId, next)
      .catch((error: unknown) => {
        console.error("changing the screen share quality failed", error);
        setChoice(before);
        announce(t("changeFailed"), "assertive");
      });
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Tab") {
      setOpen(false);
      return;
    }
    const items = [...event.currentTarget.querySelectorAll<HTMLElement>(ITEM)];
    const at = items.findIndex((item) => item === document.activeElement);
    const target =
      event.key === "ArrowDown"
        ? items[(at + 1) % items.length]
        : event.key === "ArrowUp"
          ? items[(at - 1 + items.length) % items.length]
          : event.key === "Home"
            ? items[0]
            : event.key === "End"
              ? items[items.length - 1]
              : undefined;
    if (target === undefined) return;
    event.preventDefault();
    target.focus();
  };

  const checked = choice === null ? null : checkedRow(choice);
  const pickRow = (row: QualityRow) => {
    if (choice !== null) apply(chooseRow(choice, row, settingsFps));
  };

  return (
    <span className="share-quality">
      <button
        ref={button}
        type="button"
        className="button sharer-bar__button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        disabled={choice === null}
        onClick={() => {
          setOpen((value) => !value);
        }}
      >
        <GaugeIcon />
        {t("liveBar.quality")}
        <ChevronDownIcon />
      </button>
      {open && choice !== null && (
        <div ref={popover} className="share-quality__popover">
          <div
            id={menuId}
            role="menu"
            tabIndex={-1}
            aria-label={t("qualityMenu.title")}
            className="share-quality__menu"
            onKeyDown={onKeyDown}
          >
            <div role="group" aria-labelledby={qualityId}>
              <p id={qualityId} className="share-quality__title">
                {t("qualityMenu.title")}
              </p>
              <Item
                checked={checked === "auto"}
                label={t("qualityMenu.auto")}
                hint={t("qualityMenu.autoHint")}
                onChoose={() => {
                  pickRow("auto");
                }}
              />
              {QUALITY_PRESETS.map((preset, index) => (
                <Item
                  key={`${preset.resolution}-${preset.fps}`}
                  checked={checked === index}
                  label={t("qualityMenu.preset", {
                    resolution: `${preset.resolution}p`,
                    fps: preset.fps,
                  })}
                  hint={t("qualityMenu.bitrate", { mbit: preset.mbit })}
                  onChoose={() => {
                    pickRow(index);
                  }}
                />
              ))}
            </div>
            <div role="separator" className="share-quality__separator" />
            <div role="group" aria-labelledby={optimizeId}>
              <p id={optimizeId} className="share-quality__title">
                {t("qualityMenu.optimize")}
              </p>
              {OPTIMIZE_CHOICES.map((optimize) => (
                <Item
                  key={optimize}
                  checked={choice.optimize === optimize}
                  label={t(
                    optimize === "text"
                      ? "qualityMenu.optimizeSharp"
                      : "qualityMenu.optimizeSmooth",
                  )}
                  onChoose={() => {
                    apply({ ...choice, optimize });
                  }}
                />
              ))}
            </div>
          </div>
          <p className="share-quality__note">{t("qualityMenu.note")}</p>
        </div>
      )}
    </span>
  );
}

function Item({
  checked,
  label,
  hint,
  onChoose,
}: {
  checked: boolean;
  label: string;
  hint?: string;
  onChoose: () => void;
}): ReactNode {
  return (
    <button
      type="button"
      role="menuitemradio"
      aria-checked={checked}
      tabIndex={-1}
      className="share-quality__item"
      onClick={onChoose}
    >
      {checked && <CheckIcon />}
      <span className="share-quality__label">{label}</span>
      {hint !== undefined && <span className="share-quality__hint">{hint}</span>}
    </button>
  );
}
