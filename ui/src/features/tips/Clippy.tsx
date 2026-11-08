/**
 * Clippy, the tips helper (#637): a cartoon lighter with a face, in Mumcord's colours. Its own
 * component, so the tour does not depend on how the character looks. Decorative only: the
 * speech bubble carries everything a screen reader needs.
 */
export function Clippy() {
  return (
    <svg
      className="clippy"
      viewBox="0 0 120 170"
      aria-hidden="true"
      focusable="false"
      data-testid="clippy"
    >
      <defs>
        <linearGradient id="clippy-body" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#ab7bfe" />
          <stop offset="1" stopColor="#7126fe" />
        </linearGradient>
      </defs>
      <g className="clippy__flame">
        <path
          d="M50 6c9 12 16 20 16 30a16 16 0 0 1-32 0c0-10 7-18 16-30z"
          fill="#fb7f88"
          stroke="#26026d"
          strokeWidth="3"
          strokeLinejoin="round"
        />
        <path d="M50 22c5 7 8 10 8 15a8 8 0 0 1-16 0c0-5 3-8 8-15z" fill="#fdb558" />
      </g>
      {/* The metal hood with its two slots, and the striker wheel beside the flame. */}
      <rect
        x="34"
        y="50"
        width="52"
        height="26"
        rx="6"
        fill="#e9e4f7"
        stroke="#26026d"
        strokeWidth="3"
      />
      <path d="M44 58v10M52 58v10" stroke="#26026d" strokeWidth="3" strokeLinecap="round" />
      <circle cx="73" cy="50" r="9" fill="#b3abd1" stroke="#26026d" strokeWidth="3" />
      <path d="M67 50h12M73 44v12" stroke="#26026d" strokeWidth="2" strokeLinecap="round" />
      <rect
        x="26"
        y="74"
        width="68"
        height="90"
        rx="20"
        fill="url(#clippy-body)"
        stroke="#26026d"
        strokeWidth="3"
      />
      <rect x="33" y="84" width="7" height="66" rx="3.5" fill="#ffffff" opacity="0.3" />
      <path
        d="M41 92q7-6 14-1M65 91q7-5 14 1"
        fill="none"
        stroke="#26026d"
        strokeWidth="3"
        strokeLinecap="round"
      />
      <g className="clippy__eyes">
        <ellipse
          cx="49"
          cy="106"
          rx="8"
          ry="10"
          fill="#ffffff"
          stroke="#26026d"
          strokeWidth="2.5"
        />
        <ellipse
          cx="71"
          cy="106"
          rx="8"
          ry="10"
          fill="#ffffff"
          stroke="#26026d"
          strokeWidth="2.5"
        />
        <circle cx="51" cy="108" r="4" fill="#26026d" />
        <circle cx="69" cy="108" r="4" fill="#26026d" />
        <circle cx="52.5" cy="106.5" r="1.4" fill="#ffffff" />
        <circle cx="70.5" cy="106.5" r="1.4" fill="#ffffff" />
      </g>
      <ellipse cx="39" cy="125" rx="5" ry="3.2" fill="#fb7f88" opacity="0.6" />
      <ellipse cx="81" cy="125" rx="5" ry="3.2" fill="#fb7f88" opacity="0.6" />
      <path
        d="M51 125q9 9 18 0"
        fill="none"
        stroke="#26026d"
        strokeWidth="3"
        strokeLinecap="round"
      />
    </svg>
  );
}
