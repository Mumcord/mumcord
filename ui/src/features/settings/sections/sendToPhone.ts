import { useCallback, useEffect, useRef, useState } from "react";
import { getApi, type PhoneTransfer, type PhoneTransferQr } from "../../../api";

/**
 * How long each QR code stays up before the next one. A phone camera needs 100 to 300 ms for a
 * code of this density, so every showing gives it two or three tries, and a transfer of seven
 * codes still comes round about twenty times within its two minutes.
 */
export const DWELL_MS = 800;

/** Blank modules around a QR code; scanners need four. */
export const QUIET_ZONE = 4;

/** Side of the QR picture in CSS pixels that the module size is fitted into. */
const QR_BOX_PX = 340;
const MIN_MODULE_PX = 4;
const MAX_MODULE_PX = 8;

/** Whole CSS pixels per module: fractions would blur the edges and scanners misread them. */
export function modulePx(size: number): number {
  const fitted = Math.floor(QR_BOX_PX / (size + 2 * QUIET_ZONE));
  return Math.min(MAX_MODULE_PX, Math.max(MIN_MODULE_PX, fitted));
}

/** The dark modules as one SVG path of row runs, in module units, offset by the quiet zone. */
export function qrPath(qr: PhoneTransferQr): string {
  const runs: string[] = [];
  qr.rows.forEach((row, y) => {
    let x = 0;
    while (x < row.length) {
      if (row[x] !== "1") {
        x += 1;
        continue;
      }
      let end = x;
      while (end < row.length && row[end] === "1") end += 1;
      runs.push(
        `M${String(x + QUIET_ZONE)} ${String(y + QUIET_ZONE)}h${String(end - x)}v1h-${String(end - x)}z`,
      );
      x = end;
    }
  });
  return runs.join("");
}

/** The typed code in two halves, as the phone shows its input ("4182 0735"). */
export function groupCode(code: string): string {
  const half = Math.ceil(code.length / 2);
  return `${code.slice(0, half)} ${code.slice(half)}`;
}

/** The code for screen readers: digit by digit, not as one large number. */
export function spokenCode(code: string): string {
  return code.replace(/\d(?=\d)/g, "$& ");
}

/** Remaining time as minutes and seconds ("1:42"). */
export function formatLeft(seconds: number): string {
  const whole = Math.max(0, Math.ceil(seconds));
  return `${String(Math.floor(whole / 60))}:${String(whole % 60).padStart(2, "0")}`;
}

export interface Selection {
  identityIds: readonly number[];
  includeServers: boolean;
}

export function isEmpty(selection: Selection): boolean {
  return selection.identityIds.length === 0 && !selection.includeServers;
}

export type Phase =
  | { kind: "select" }
  | { kind: "preparing" }
  | { kind: "failed"; error: unknown }
  | {
      kind: "showing";
      transfer: PhoneTransfer;
      /** Index of the QR code on screen. */
      index: number;
      secondsLeft: number;
    };

export interface SendToPhone {
  selection: Selection;
  phase: Phase;
  /** The codes on screen replaced earlier ones: the phone has to start again. */
  renewed: boolean;
  setSelection: (selection: Selection) => void;
  /** Leaves the selection step and shows the codes. */
  start: () => void;
}

/**
 * State of the "Send to your phone" dialog (spec 05, FR-5.64): which entries are chosen, the
 * transfer on show, which of its QR codes is up and how long it has left. The transfer itself
 * lives in the backend; this hook only holds what is drawn, and lets go of it when the
 * selection changes, when the time is up and when the dialog closes.
 */
export function useSendToPhone(initial: Selection): SendToPhone {
  const [selection, setSelectionState] = useState(initial);
  const [phase, setPhase] = useState<Phase>({ kind: "select" });
  const [renewed, setRenewed] = useState(false);
  /** Counts requests, so the answer to an overtaken one is ignored. */
  const request = useRef(0);
  const deadline = useRef(0);

  const show = useCallback((wanted: Selection, replaces: boolean) => {
    request.current += 1;
    const mine = request.current;
    setPhase({ kind: "preparing" });
    setRenewed(replaces);
    getApi()
      .phoneTransferStart([...wanted.identityIds], wanted.includeServers)
      .then((transfer) => {
        if (request.current !== mine) return;
        deadline.current = Date.now() + transfer.secondsLeft * 1000;
        setPhase({ kind: "showing", transfer, index: 0, secondsLeft: transfer.secondsLeft });
      })
      .catch((error: unknown) => {
        if (request.current !== mine) return;
        setPhase({ kind: "failed", error });
      });
  }, []);

  const drop = useCallback(() => {
    request.current += 1;
    // Nothing to report: the backend also drops the transfer by itself when its time is up.
    getApi()
      .phoneTransferCancel()
      .catch((error: unknown) => {
        console.error("Dropping the phone transfer failed", error);
      });
  }, []);

  const setSelection = (next: Selection) => {
    setSelectionState(next);
    if (phase.kind === "select") return;
    if (isEmpty(next)) {
      drop();
      setRenewed(false);
      setPhase({ kind: "select" });
    } else {
      show(next, true);
    }
  };

  const showing = phase.kind === "showing";
  const total = showing ? phase.transfer.codes.length : 0;

  useEffect(() => {
    if (!showing || total < 2) return;
    const timer = window.setInterval(() => {
      setPhase((current) =>
        current.kind === "showing"
          ? { ...current, index: (current.index + 1) % current.transfer.codes.length }
          : current,
      );
    }, DWELL_MS);
    return () => {
      window.clearInterval(timer);
    };
  }, [showing, total]);

  useEffect(() => {
    if (!showing) return;
    const timer = window.setInterval(() => {
      const left = Math.ceil((deadline.current - Date.now()) / 1000);
      if (left <= 0) {
        show(selection, true);
        return;
      }
      setPhase((current) =>
        current.kind === "showing" && current.secondsLeft !== left
          ? { ...current, secondsLeft: left }
          : current,
      );
    }, 250);
    return () => {
      window.clearInterval(timer);
    };
  }, [showing, selection, show]);

  // Closing the dialog unmounts the hook: the backend forgets the transfer, and the codes held
  // in this state go with the component.
  useEffect(() => drop, [drop]);

  return {
    selection,
    phase,
    renewed,
    setSelection,
    start: () => {
      if (!isEmpty(selection)) show(selection, false);
    },
  };
}
