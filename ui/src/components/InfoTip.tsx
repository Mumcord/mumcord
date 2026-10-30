/**
 * Small "?" button that explains the control next to it. The text appears as a tooltip on hover and
 * keyboard focus (and is announced through `aria-describedby`, see `TooltipLayer`); Escape closes it.
 */
export function InfoTip({ label, text }: { label: string; text: string }) {
  return (
    <button type="button" className="info-tip" aria-label={label} data-tooltip={text}>
      ?
    </button>
  );
}
