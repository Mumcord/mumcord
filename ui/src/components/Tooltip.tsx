import "./tooltip.css";
import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { parsePlacement, placeTooltip } from "./tooltipPlacement";

/**
 * Themed tooltips (FR-2.79). Any element with `data-tooltip="…"` gets one, optionally with
 * `data-tooltip-placement="top|bottom|left|right"` (default top). One layer listens on the
 * document instead of a component per trigger, so long member lists pay nothing per row and
 * any element can opt in without a wrapper. Mount `<TooltipLayer />` once at the app root.
 *
 * The tooltip only adds a description: icon buttons still need their own `aria-label`.
 */

export const TOOLTIP_DELAY_MS = 400;

const TRIGGER = "[data-tooltip]";

interface Shown {
  trigger: HTMLElement;
  label: string;
}

function triggerOf(target: EventTarget | null): HTMLElement | null {
  return target instanceof Element ? target.closest<HTMLElement>(TRIGGER) : null;
}

function labelOf(trigger: HTMLElement): string {
  return trigger.dataset.tooltip?.trim() ?? "";
}

/** Native modal dialogs and the full-screen element sit in the top layer: the tooltip must be
 * inside them to be seen at all. */
function containerOf(trigger: HTMLElement): Element {
  const fullscreen = document.fullscreenElement;
  if (fullscreen?.contains(trigger)) return fullscreen;
  return trigger.closest("dialog[open]") ?? document.body;
}

/** Text a screen reader already speaks for the trigger, so the tooltip is not read twice. */
function alreadySpoken(trigger: HTMLElement, label: string): boolean {
  const described = (trigger.getAttribute("aria-describedby") ?? "")
    .split(/\s+/)
    .map((id) => (id ? document.getElementById(id)?.textContent.trim() : undefined));
  return [
    trigger.getAttribute("aria-label")?.trim(),
    trigger.textContent.trim(),
    ...described,
  ].includes(label);
}

function withoutToken(list: string | null, token: string): string {
  return (list ?? "")
    .split(/\s+/)
    .filter((part) => part && part !== token)
    .join(" ");
}

export function TooltipLayer() {
  const id = useId();
  const [shown, setShown] = useState<Shown | null>(null);
  const tipRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let timer: number | undefined;
    let pending: HTMLElement | null = null;
    let current: HTMLElement | null = null;
    // A click or Escape keeps that trigger's tooltip away until pointer and focus leave it.
    let dismissed: HTMLElement | null = null;
    let observer: MutationObserver | null = null;

    const cancel = () => {
      window.clearTimeout(timer);
      pending = null;
    };
    const hide = () => {
      cancel();
      observer?.disconnect();
      observer = null;
      current = null;
      setShown(null);
    };
    const show = (trigger: HTMLElement) => {
      const label = labelOf(trigger);
      if (!trigger.isConnected || label === "") return;
      current = trigger;
      setShown({ trigger, label });
      // The trigger can lose its tooltip, change it or leave the page while it is shown.
      observer = new MutationObserver(() => {
        const next = labelOf(trigger);
        if (!trigger.isConnected || next === "") hide();
        else setShown((prev) => (prev?.label === next ? prev : { trigger, label: next }));
      });
      observer.observe(document.body, {
        subtree: true,
        childList: true,
        attributes: true,
        attributeFilter: ["data-tooltip"],
      });
    };
    const schedule = (trigger: HTMLElement) => {
      if (trigger === current || trigger === pending || trigger === dismissed) return;
      if (current !== null) hide();
      cancel();
      pending = trigger;
      timer = window.setTimeout(() => {
        pending = null;
        show(trigger);
      }, TOOLTIP_DELAY_MS);
    };
    const leave = (trigger: HTMLElement, to: EventTarget | null) => {
      if (to instanceof Node && trigger.contains(to)) return;
      if (trigger === dismissed) dismissed = null;
      if (trigger === pending) cancel();
      if (trigger === current) hide();
    };

    const onPointerOver = (event: PointerEvent) => {
      const trigger = triggerOf(event.target);
      if (trigger) schedule(trigger);
    };
    const onPointerOut = (event: PointerEvent) => {
      const trigger = triggerOf(event.target);
      if (trigger) leave(trigger, event.relatedTarget);
    };
    const onFocusIn = (event: FocusEvent) => {
      const trigger = triggerOf(event.target);
      if (trigger) schedule(trigger);
    };
    const onFocusOut = (event: FocusEvent) => {
      const trigger = triggerOf(event.target);
      if (trigger) leave(trigger, event.relatedTarget);
    };
    const onPointerDown = (event: PointerEvent) => {
      const trigger = triggerOf(event.target);
      if (trigger === null) return;
      dismissed = trigger;
      if (trigger === pending) cancel();
      if (trigger === current) hide();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      const trigger = current ?? pending;
      if (trigger === null) return;
      dismissed = trigger;
      hide();
    };
    const onMove = () => {
      if (current !== null || pending !== null) hide();
    };

    document.addEventListener("pointerover", onPointerOver);
    document.addEventListener("pointerout", onPointerOut);
    document.addEventListener("focusin", onFocusIn);
    document.addEventListener("focusout", onFocusOut);
    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("keydown", onKeyDown, true);
    document.addEventListener("scroll", onMove, { capture: true, passive: true });
    window.addEventListener("resize", onMove);
    return () => {
      hide();
      document.removeEventListener("pointerover", onPointerOver);
      document.removeEventListener("pointerout", onPointerOut);
      document.removeEventListener("focusin", onFocusIn);
      document.removeEventListener("focusout", onFocusOut);
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("keydown", onKeyDown, true);
      document.removeEventListener("scroll", onMove, { capture: true });
      window.removeEventListener("resize", onMove);
    };
  }, []);

  useLayoutEffect(() => {
    const tip = tipRef.current;
    if (shown === null || tip === null) return;
    const { trigger, label } = shown;
    // Measured at 0/0 first: a transformed or filtered ancestor (glass, a dialog) moves the
    // origin of position: fixed, and this offset corrects for it.
    tip.style.left = "0px";
    tip.style.top = "0px";
    const origin = tip.getBoundingClientRect();
    const root = document.documentElement;
    const place = placeTooltip(
      trigger.getBoundingClientRect(),
      { width: origin.width, height: origin.height },
      {
        width: root.clientWidth || window.innerWidth,
        height: root.clientHeight || window.innerHeight,
      },
      parsePlacement(trigger.dataset.tooltipPlacement),
    );
    tip.style.left = `${String(place.left - origin.left)}px`;
    tip.style.top = `${String(place.top - origin.top)}px`;
    tip.dataset.placement = place.placement;

    if (alreadySpoken(trigger, label)) return;
    trigger.setAttribute(
      "aria-describedby",
      [withoutToken(trigger.getAttribute("aria-describedby"), id), id].filter(Boolean).join(" "),
    );
    return () => {
      const rest = withoutToken(trigger.getAttribute("aria-describedby"), id);
      if (rest) trigger.setAttribute("aria-describedby", rest);
      else trigger.removeAttribute("aria-describedby");
    };
  }, [shown, id]);

  if (shown === null) return null;
  const inModal = shown.trigger.closest('dialog, [aria-modal="true"]') !== null;
  return createPortal(
    <div
      ref={tipRef}
      id={id}
      role="tooltip"
      className="tooltip"
      data-in-modal={inModal || undefined}
    >
      {shown.label}
    </div>,
    containerOf(shown.trigger),
  );
}
