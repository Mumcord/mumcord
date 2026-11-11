import { useEffect, useId, useRef, type CSSProperties } from "react";
import "./aaron.css";

/**
 * Aaron, Mumcord's axolotl mascot (#637). Decorative only: whatever he says lives in the
 * surrounding UI, so screen readers skip him. The eyes follow the mouse; the eased gaze runs in a
 * requestAnimationFrame loop that writes straight to the SVG, so moving the mouse never
 * re-renders React.
 */
export type AaronPose = "idle" | "point-left" | "point-right" | "point-up";

interface AaronProps {
  pose: AaronPose;
  talking: boolean;
  className?: string;
}

type Side = "left" | "right";

interface PointPose {
  side: Side;
  /** Arm angle in degrees above the horizontal, away from the body. */
  angle: number;
  /** Body lean in degrees, toward the side he points at. */
  lean: number;
  /** Where the eyes look, as a direction in view box units. */
  gaze: readonly [number, number];
}

const POINT_POSES: Record<Exclude<AaronPose, "idle">, PointPose> = {
  "point-left": { side: "left", angle: 12, lean: -6, gaze: [-1, -0.3] },
  "point-right": { side: "right", angle: 12, lean: 6, gaze: [1, -0.3] },
  // Pointing up uses the left arm with the forearm bent up from the elbow, like the mockup.
  "point-up": { side: "left", angle: 80, lean: -1, gaze: [-0.2, -1] },
};

/** Shoulder and elbow of each pointing arm, in view box units. */
const ARM_JOINTS: Record<Side, { shoulder: string; elbow: string }> = {
  left: { shoulder: "78px 146.5px", elbow: "59px 145px" },
  right: { shoulder: "122px 146.5px", elbow: "141px 145px" },
};

/** Past this angle the upper arm stops rising and the forearm bends up instead. */
const UPPER_ARM_LIMIT = 6;

const VIEW_BOX = { x: -12, y: -8, width: 224, height: 212 };
const VIEW_BOX_ATTRIBUTE = "-12 -8 224 212";
const EYE_CENTRES = [
  { x: 74, y: 98 },
  { x: 126, y: 98 },
] as const;
/** How far a pupil may move from the centre of its eye before it would leave the eye white. */
const PUPIL_RANGE = 3.2;
/** How far the face slides toward the gaze, which reads as the head turning. */
const FACE_SHIFT = 3.5;
const REDUCE_MOTION = "(prefers-reduced-motion: reduce)";

interface Gaze {
  centre: { x: number; y: number };
  x: number;
  y: number;
  targetX: number;
  targetY: number;
}

/** Maps a point on screen into the SVG's view box (the root uses the default "meet" scaling). */
function toViewBox(svg: SVGSVGElement, clientX: number, clientY: number) {
  const rect = svg.getBoundingClientRect();
  if (rect.width === 0 || rect.height === 0) return null;
  const scale = Math.min(rect.width / VIEW_BOX.width, rect.height / VIEW_BOX.height);
  const offsetX = (rect.width - VIEW_BOX.width * scale) / 2;
  const offsetY = (rect.height - VIEW_BOX.height * scale) / 2;
  return {
    x: VIEW_BOX.x + (clientX - rect.left - offsetX) / scale,
    y: VIEW_BOX.y + (clientY - rect.top - offsetY) / scale,
  };
}

/** Offset of a pupil looking in direction (dx, dy): full range once the target is a bit away. */
function pupilOffset(dx: number, dy: number, reach: number) {
  const distance = Math.hypot(dx, dy);
  if (distance === 0) return { x: 0, y: 0 };
  const length = (Math.min(1, distance / reach) * PUPIL_RANGE) / distance;
  return { x: dx * length, y: dy * length };
}

interface ArmStyles {
  arms: Record<Side, CSSProperties | undefined>;
  forearms: Record<Side, CSSProperties | undefined>;
  lean: CSSProperties | undefined;
}

function armStyles(pose: AaronPose): ArmStyles {
  const none: Record<Side, CSSProperties | undefined> = { left: undefined, right: undefined };
  if (pose === "idle") return { arms: none, forearms: none, lean: undefined };
  const { side, angle, lean } = POINT_POSES[pose];
  const upper = Math.min(angle, UPPER_ARM_LIMIT);
  // SVG rotation is clockwise, so raising the left arm is positive and the right arm negative.
  const sign = side === "left" ? 1 : -1;
  return {
    arms: {
      ...none,
      [side]: {
        transformOrigin: ARM_JOINTS[side].shoulder,
        transform: `rotate(${String(sign * upper)}deg)`,
      },
    },
    forearms: {
      ...none,
      [side]: {
        transformOrigin: ARM_JOINTS[side].elbow,
        transform: `rotate(${String(sign * (angle - upper))}deg)`,
      },
    },
    lean: { transformOrigin: "100px 190px", transform: `rotate(${String(lean)}deg)` },
  };
}

/* Shapes of each limb, stacked in several layers (outline, skin, shading) below. */
const REST_LEFT = (
  <>
    <path d="M73.91 135.91 L72.75 137.24 L71.65 138.58 L70.63 139.94 L69.68 141.31 L68.8 142.7 L68 144.1 L67.27 145.51 L66.62 146.94 L66.06 148.39 L65.58 149.85 L65.19 151.32 L64.88 152.79 L64.67 154.27 L64.56 155.76 L64.53 157.23 L64.6 158.7 L64.76 160.16 L65.02 161.6 L65.36 163.02 L65.79 164.42 L76.21 161.58 L76.11 160.78 L76.06 160 L76.07 159.24 L76.12 158.5 L76.22 157.77 L76.36 157.04 L76.56 156.33 L76.8 155.61 L77.08 154.88 L77.42 154.15 L77.81 153.41 L78.26 152.66 L78.76 151.89 L79.32 151.1 L79.95 150.3 L80.64 149.49 L81.4 148.66 L82.23 147.82 L83.12 146.96 L84.09 146.09 Z" />
    <circle cx="79" cy="141" r="7.2" />
    <circle cx="71" cy="163" r="5.4" />
    <ellipse cx="71.89" cy="166.28" rx="6.2" ry="5.6" transform="rotate(74.74 71.89 166.28)" />
    <path d="M72.83 169.02 L73.77 169.55 L74.7 170.07 L75.63 170.59 L76.57 171.11 L77.5 171.64 L78.44 172.16 L80.61 167.43 L79.61 167.06 L78.6 166.69 L77.6 166.32 L76.6 165.95 L75.59 165.58 L74.59 165.21 Z" />
    <circle cx="73.71" cy="167.12" r="2.1" />
    <circle cx="79.52" cy="169.8" r="2.6" />
    <path d="M71.19 169.09 L71.68 170.04 L72.18 170.99 L72.68 171.93 L73.18 172.88 L73.68 173.82 L74.18 174.77 L78.58 172 L77.94 171.14 L77.3 170.28 L76.66 169.42 L76.02 168.57 L75.38 167.71 L74.74 166.85 Z" />
    <circle cx="72.96" cy="167.97" r="2.1" />
    <circle cx="76.38" cy="173.38" r="2.6" />
    <path d="M69.77 168.25 L69.67 169.32 L69.57 170.38 L69.48 171.45 L69.38 172.51 L69.28 173.58 L69.19 174.65 L74.39 174.71 L74.32 173.65 L74.25 172.58 L74.18 171.51 L74.11 170.44 L74.04 169.37 L73.97 168.31 Z" />
    <circle cx="71.87" cy="168.28" r="2.1" />
    <circle cx="71.79" cy="174.68" r="2.6" />
    <path d="M69.05 166.83 L68.4 167.68 L67.75 168.53 L67.11 169.38 L66.46 170.23 L65.81 171.09 L65.16 171.94 L69.54 174.75 L70.04 173.81 L70.55 172.87 L71.06 171.92 L71.57 170.98 L72.07 170.04 L72.58 169.1 Z" />
    <circle cx="70.81" cy="167.96" r="2.1" />
    <circle cx="67.35" cy="173.35" r="2.6" />
  </>
);

const REST_RIGHT = (
  <>
    <path d="M115.91 146.09 L116.88 146.96 L117.77 147.82 L118.6 148.66 L119.36 149.49 L120.05 150.3 L120.68 151.1 L121.24 151.89 L121.74 152.66 L122.19 153.41 L122.58 154.15 L122.92 154.88 L123.2 155.61 L123.44 156.33 L123.64 157.04 L123.78 157.77 L123.88 158.5 L123.93 159.24 L123.94 160 L123.89 160.78 L123.79 161.58 L134.21 164.42 L134.64 163.02 L134.98 161.6 L135.24 160.16 L135.4 158.7 L135.47 157.23 L135.44 155.76 L135.33 154.27 L135.12 152.79 L134.81 151.32 L134.42 149.85 L133.94 148.39 L133.38 146.94 L132.73 145.51 L132 144.1 L131.2 142.7 L130.32 141.31 L129.37 139.94 L128.35 138.58 L127.25 137.24 L126.09 135.91 Z" />
    <circle cx="121" cy="141" r="7.2" />
    <circle cx="129" cy="163" r="5.4" />
    <ellipse cx="128.11" cy="166.28" rx="6.2" ry="5.6" transform="rotate(105.26 128.11 166.28)" />
    <path d="M127.52 169.12 L128.06 170.04 L128.6 170.97 L129.14 171.89 L129.68 172.82 L130.22 173.74 L130.76 174.66 L135.03 171.7 L134.35 170.87 L133.68 170.04 L133 169.21 L132.32 168.38 L131.65 167.56 L130.97 166.73 Z" />
    <circle cx="129.25" cy="167.92" r="2.1" />
    <circle cx="132.89" cy="173.18" r="2.6" />
    <path d="M126.07 168.34 L126.02 169.41 L125.97 170.48 L125.91 171.55 L125.86 172.62 L125.81 173.69 L125.76 174.76 L130.96 174.6 L130.84 173.53 L130.73 172.47 L130.61 171.41 L130.5 170.34 L130.38 169.28 L130.27 168.22 Z" />
    <circle cx="128.17" cy="168.28" r="2.1" />
    <circle cx="128.36" cy="174.68" r="2.6" />
    <path d="M125.27 166.9 L124.65 167.77 L124.02 168.64 L123.4 169.51 L122.77 170.37 L122.15 171.24 L121.52 172.11 L125.97 174.81 L126.45 173.85 L126.93 172.9 L127.42 171.94 L127.9 170.99 L128.38 170.03 L128.86 169.08 Z" />
    <circle cx="127.07" cy="167.99" r="2.1" />
    <circle cx="123.75" cy="173.46" r="2.6" />
    <path d="M125.37 165.3 L124.38 165.71 L123.39 166.12 L122.4 166.52 L121.41 166.93 L120.42 167.33 L119.43 167.74 L121.77 172.38 L122.69 171.83 L123.6 171.27 L124.52 170.72 L125.43 170.16 L126.35 169.61 L127.26 169.06 Z" />
    <circle cx="126.32" cy="167.18" r="2.1" />
    <circle cx="120.6" cy="170.06" r="2.6" />
  </>
);

const UPPER_LEFT = (
  <>
    <path d="M78.15 139.3 L77.24 139.33 L76.33 139.36 L75.42 139.39 L74.51 139.4 L73.6 139.41 L72.68 139.42 L71.77 139.42 L70.85 139.42 L69.94 139.41 L69.02 139.4 L68.11 139.38 L67.19 139.35 L66.27 139.32 L65.35 139.28 L64.43 139.24 L63.51 139.2 L62.59 139.14 L61.67 139.09 L60.75 139.02 L59.83 138.96 L58.17 151.04 L59.15 151.23 L60.13 151.41 L61.11 151.59 L62.09 151.76 L63.07 151.92 L64.05 152.08 L65.03 152.23 L66.01 152.38 L66.99 152.52 L67.98 152.65 L68.96 152.78 L69.95 152.91 L70.93 153.03 L71.92 153.14 L72.9 153.25 L73.89 153.35 L74.88 153.45 L75.87 153.54 L76.86 153.62 L77.85 153.7 Z" />
    <circle cx="78" cy="146.5" r="7.2" />
    <circle cx="59" cy="145" r="6.1" />
  </>
);

const UPPER_RIGHT = (
  <>
    <path d="M122.15 153.7 L123.14 153.62 L124.13 153.54 L125.12 153.45 L126.11 153.35 L127.1 153.25 L128.08 153.14 L129.07 153.03 L130.05 152.91 L131.04 152.78 L132.02 152.65 L133.01 152.52 L133.99 152.38 L134.97 152.23 L135.95 152.08 L136.93 151.92 L137.91 151.76 L138.89 151.59 L139.87 151.41 L140.85 151.23 L141.83 151.04 L140.17 138.96 L139.25 139.02 L138.33 139.09 L137.41 139.14 L136.49 139.2 L135.57 139.24 L134.65 139.28 L133.73 139.32 L132.81 139.35 L131.89 139.38 L130.98 139.4 L130.06 139.41 L129.15 139.42 L128.23 139.42 L127.32 139.42 L126.4 139.41 L125.49 139.4 L124.58 139.39 L123.67 139.36 L122.76 139.33 L121.85 139.3 Z" />
    <circle cx="122" cy="146.5" r="7.2" />
    <circle cx="141" cy="145" r="6.1" />
  </>
);

const FORE_LEFT = (
  <>
    <path d="M59.81 138.95 L59.07 138.89 L58.33 138.83 L57.6 138.77 L56.86 138.7 L56.13 138.63 L55.39 138.56 L54.65 138.49 L53.91 138.41 L53.18 138.33 L52.44 138.25 L51.7 138.17 L50.96 138.08 L50.22 137.99 L49.48 137.9 L48.74 137.81 L48 137.71 L47.26 137.61 L46.52 137.51 L45.78 137.41 L45.04 137.3 L42.96 147.7 L43.72 147.89 L44.48 148.08 L45.24 148.26 L46 148.45 L46.76 148.63 L47.52 148.81 L48.28 148.98 L49.04 149.16 L49.8 149.33 L50.56 149.5 L51.32 149.66 L52.09 149.83 L52.85 149.99 L53.61 150.15 L54.37 150.3 L55.14 150.46 L55.9 150.61 L56.67 150.76 L57.43 150.9 L58.19 151.05 Z" />
    <circle cx="59" cy="145" r="6.1" />
    <circle cx="44" cy="142.5" r="5.3" />
    <ellipse cx="39.86" cy="141.81" rx="7.6" ry="6.8" transform="rotate(-170.54 39.86 141.81)" />
    <path d="M37.79 136.8 L35.68 136.3 L33.57 135.8 L31.46 135.29 L29.35 134.79 L27.24 134.29 L25.13 133.78 L23.74 140.24 L25.88 140.64 L28.01 141.05 L30.14 141.45 L32.27 141.86 L34.4 142.27 L36.53 142.67 Z" />
    <circle cx="37.16" cy="139.74" r="3" />
    <circle cx="24.44" cy="137.01" r="3.3" />
    <circle cx="34.04" cy="145.1" r="3.2" />
    <circle cx="37.36" cy="147.07" r="3.2" />
    <circle cx="40.98" cy="147.27" r="3" />
  </>
);

const FORE_RIGHT = (
  <>
    <path d="M141.81 151.05 L142.57 150.9 L143.33 150.76 L144.1 150.61 L144.86 150.46 L145.63 150.3 L146.39 150.15 L147.15 149.99 L147.91 149.83 L148.68 149.66 L149.44 149.5 L150.2 149.33 L150.96 149.16 L151.72 148.98 L152.48 148.81 L153.24 148.63 L154 148.45 L154.76 148.26 L155.52 148.08 L156.28 147.89 L157.04 147.7 L154.96 137.3 L154.22 137.41 L153.48 137.51 L152.74 137.61 L152 137.71 L151.26 137.81 L150.52 137.9 L149.78 137.99 L149.04 138.08 L148.3 138.17 L147.56 138.25 L146.82 138.33 L146.09 138.41 L145.35 138.49 L144.61 138.56 L143.87 138.63 L143.14 138.7 L142.4 138.77 L141.67 138.83 L140.93 138.89 L140.19 138.95 Z" />
    <circle cx="141" cy="145" r="6.1" />
    <circle cx="156" cy="142.5" r="5.3" />
    <ellipse cx="160.14" cy="141.81" rx="7.6" ry="6.8" transform="rotate(-9.46 160.14 141.81)" />
    <path d="M163.47 142.67 L165.6 142.27 L167.73 141.86 L169.86 141.45 L171.99 141.05 L174.12 140.64 L176.26 140.24 L174.87 133.78 L172.76 134.29 L170.65 134.79 L168.54 135.29 L166.43 135.8 L164.32 136.3 L162.21 136.8 Z" />
    <circle cx="162.84" cy="139.74" r="3" />
    <circle cx="175.56" cy="137.01" r="3.3" />
    <circle cx="165.96" cy="145.1" r="3.2" />
    <circle cx="162.64" cy="147.07" r="3.2" />
    <circle cx="159.02" cy="147.27" r="3" />
  </>
);

const GILL_1 = (
  <>
    <circle cx="10.2" cy="-6.5" r="4.2" />
    <circle cx="10.2" cy="6.5" r="4.2" />
    <circle cx="17.68" cy="-6.5" r="4.2" />
    <circle cx="17.68" cy="6.5" r="4.2" />
    <circle cx="25.16" cy="-6.5" r="4.2" />
    <circle cx="25.16" cy="6.5" r="4.2" />
    <path d="M0 -7 C15.3 -10 28.9 -7 34 0 C28.9 7 15.3 10 0 7 Z" />
  </>
);

const GILL_2 = (
  <>
    <circle cx="10.8" cy="-6.5" r="4.2" />
    <circle cx="10.8" cy="6.5" r="4.2" />
    <circle cx="18.72" cy="-6.5" r="4.2" />
    <circle cx="18.72" cy="6.5" r="4.2" />
    <circle cx="26.64" cy="-6.5" r="4.2" />
    <circle cx="26.64" cy="6.5" r="4.2" />
    <path d="M0 -7 C16.2 -10 30.599999999999998 -7 36 0 C30.599999999999998 7 16.2 10 0 7 Z" />
  </>
);

const GILL_3 = (
  <>
    <circle cx="9" cy="-6.5" r="4.2" />
    <circle cx="9" cy="6.5" r="4.2" />
    <circle cx="15.6" cy="-6.5" r="4.2" />
    <circle cx="15.6" cy="6.5" r="4.2" />
    <circle cx="22.2" cy="-6.5" r="4.2" />
    <circle cx="22.2" cy="6.5" r="4.2" />
    <path d="M0 -7 C13.5 -10 25.5 -7 30 0 C25.5 7 13.5 10 0 7 Z" />
  </>
);

export function Aaron({ pose, talking, className }: AaronProps) {
  // Gradients, clips and masks are looked up by id document-wide, so every Aaron on screen needs
  // its own. React's ids contain characters that are awkward inside url(#…), hence the cleanup.
  const uid = `aaron${useId().replace(/[^a-zA-Z0-9]/g, "")}`;
  const id = (name: string) => `${uid}-${name}`;
  const ref = (name: string) => `url(#${uid}-${name})`;

  const svgRef = useRef<SVGSVGElement>(null);
  const faceRef = useRef<SVGGElement>(null);
  const pupilLeftRef = useRef<SVGGElement>(null);
  const pupilRightRef = useRef<SVGGElement>(null);
  // Kept across pose changes so the eyes glide on from where they are.
  const gazes = useRef<Gaze[]>(
    EYE_CENTRES.map((centre) => ({ centre, x: 0, y: 0, targetX: 0, targetY: 0 })),
  );
  const pointer = useRef<{ x: number; y: number } | null>(null);

  useEffect(() => {
    const svg = svgRef.current;
    const face = faceRef.current;
    const pupils = [pupilLeftRef.current, pupilRightRef.current];
    if (!svg || !face) return;
    const motion = typeof matchMedia === "function" ? matchMedia(REDUCE_MOTION) : null;
    let frame = 0;

    const tick = () => {
      frame = 0;
      const ease = motion?.matches ? 1 : 0.25;
      let moving = false;
      for (const [index, gaze] of gazes.current.entries()) {
        gaze.x += (gaze.targetX - gaze.x) * ease;
        gaze.y += (gaze.targetY - gaze.y) * ease;
        if (Math.abs(gaze.targetX - gaze.x) + Math.abs(gaze.targetY - gaze.y) > 0.02) {
          moving = true;
        }
        pupils[index]?.setAttribute(
          "transform",
          `translate(${gaze.x.toFixed(2)} ${gaze.y.toFixed(2)})`,
        );
      }
      const [left, right] = gazes.current;
      if (left && right) {
        const shift = FACE_SHIFT / PUPIL_RANGE;
        const x = ((left.x + right.x) / 2) * shift;
        const y = ((left.y + right.y) / 2) * shift * 0.6;
        face.style.transform = `translate(${x.toFixed(2)}px, ${y.toFixed(2)}px)`;
      }
      if (moving) frame = requestAnimationFrame(tick);
    };

    const aim = () => {
      const target = pointer.current && toViewBox(svg, pointer.current.x, pointer.current.y);
      for (const gaze of gazes.current) {
        const { centre } = gaze;
        // While pointing he looks where he points; otherwise at the cursor, or straight ahead.
        const offset =
          pose !== "idle"
            ? pupilOffset(...POINT_POSES[pose].gaze, 1)
            : target
              ? pupilOffset(target.x - centre.x, target.y - centre.y, 40)
              : { x: 0, y: 0 };
        gaze.targetX = offset.x;
        gaze.targetY = offset.y;
      }
      if (!frame) frame = requestAnimationFrame(tick);
    };

    const onMove = (event: PointerEvent) => {
      pointer.current = { x: event.clientX, y: event.clientY };
      aim();
    };
    // No related target means the pointer left the window, not just one element.
    const onOut = (event: PointerEvent) => {
      if (event.relatedTarget !== null) return;
      pointer.current = null;
      aim();
    };

    aim();
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerout", onOut);
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerout", onOut);
      cancelAnimationFrame(frame);
    };
  }, [pose]);

  // The mouth opens in syllable-like bursts. Written as a CSS variable, so no re-renders.
  useEffect(() => {
    const svg = svgRef.current;
    if (!svg || !talking) return;
    if (typeof matchMedia === "function" && matchMedia(REDUCE_MOTION).matches) {
      svg.style.setProperty("--aaron-mouth", "0.8");
      return;
    }
    let beat = 0;
    const timer = window.setInterval(() => {
      beat += 1;
      const syllable = Math.abs(Math.sin(beat * 0.9)) * 0.7 + Math.random() * 0.3;
      svg.style.setProperty("--aaron-mouth", (0.25 + syllable * 0.75).toFixed(2));
    }, 90);
    return () => {
      window.clearInterval(timer);
    };
  }, [talking]);

  const { arms, forearms, lean } = armStyles(pose);
  const classes = ["aaron"];
  if (talking) classes.push("aaron--talking");
  if (pose !== "idle") classes.push("aaron--pointing");
  if (pose === "point-right") classes.push("aaron--point-right");
  if (className) classes.push(className);

  return (
    <svg
      ref={svgRef}
      className={classes.join(" ")}
      viewBox={VIEW_BOX_ATTRIBUTE}
      aria-hidden="true"
      focusable="false"
      data-testid="aaron"
      data-pose={pose}
    >
      <defs>
        <radialGradient id={id("white")} cx="0.45" cy="0.38" r="0.7">
          <stop offset="0" stopColor="#ffffff" />
          <stop offset="0.62" stopColor="#f8f4ff" />
          <stop offset="1" stopColor="#cdbff2" />
        </radialGradient>
        <radialGradient id={id("iris")} cx="0.5" cy="0.62" r="0.6">
          <stop offset="0" stopColor="#ff9fcf" />
          <stop offset="0.7" stopColor="#c2408a" />
          <stop offset="1" stopColor="#5a1440" />
        </radialGradient>
        <linearGradient id={id("eyeshade")} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#2a1260" stopOpacity="0.32" />
          <stop offset="0.38" stopColor="#2a1260" stopOpacity="0" />
        </linearGradient>
        <radialGradient id={id("shadow")} cx="0.5" cy="0.5" r="0.5">
          <stop offset="0" stopColor="#140a33" stopOpacity="0.55" />
          <stop offset="1" stopColor="#140a33" stopOpacity="0" />
        </radialGradient>
        <radialGradient id={id("skin")} gradientUnits="userSpaceOnUse" cx="82" cy="72" r="120">
          <stop offset="0" stopColor="#ffe8f3" />
          <stop offset="0.4" stopColor="#ffb6d8" />
          <stop offset="0.78" stopColor="#e48fd6" />
          <stop offset="1" stopColor="#a77bf0" />
        </radialGradient>
        <radialGradient id={id("belly")} cx="0.5" cy="0.35" r="0.7">
          <stop offset="0" stopColor="#fff6fa" />
          <stop offset="1" stopColor="#ffd4e8" />
        </radialGradient>
        <linearGradient id={id("gill")} x1="0" y1="0" x2="1" y2="0">
          <stop offset="0" stopColor="#ff7fae" />
          <stop offset="0.55" stopColor="#ff9a8a" />
          <stop offset="1" stopColor="#b48cff" />
        </linearGradient>
        <mask
          id={id("arml-fade")}
          maskUnits="userSpaceOnUse"
          x="-20"
          y="-20"
          width="240"
          height="240"
        >
          <rect x="-20" y="-20" width="240" height="240" fill="#fff" />
          <circle cx="85.5" cy="141" r="9.5" fill="#000" />
        </mask>
        <mask
          id={id("armr-fade")}
          maskUnits="userSpaceOnUse"
          x="-20"
          y="-20"
          width="240"
          height="240"
        >
          <rect x="-20" y="-20" width="240" height="240" fill="#fff" />
          <circle cx="114.5" cy="141" r="9.5" fill="#000" />
        </mask>
        <mask
          id={id("pul-fade")}
          maskUnits="userSpaceOnUse"
          x="-20"
          y="-20"
          width="240"
          height="240"
        >
          <rect x="-20" y="-20" width="240" height="240" fill="#fff" />
          <circle cx="84.5" cy="146.5" r="11" fill="#000" />
        </mask>
        <mask
          id={id("pur-fade")}
          maskUnits="userSpaceOnUse"
          x="-20"
          y="-20"
          width="240"
          height="240"
        >
          <rect x="-20" y="-20" width="240" height="240" fill="#fff" />
          <circle cx="115.5" cy="146.5" r="11" fill="#000" />
        </mask>
      </defs>
      <ellipse className="aaron__shadow" cx="100" cy="190" rx="50" ry="7" fill={ref("shadow")} />
      <g className="aaron__lean" style={lean}>
        <g className="aaron__hop">
          <g className="aaron__float">
            <g className="aaron__breathe">
              <path
                d="M124 168 C152 176 176 168 188 148 C182 140 172 146 160 150 C148 154 136 154 124 150 Z"
                fill={ref("skin")}
                stroke="#2b1250"
                strokeWidth="3"
                strokeLinejoin="round"
              />
              <path
                d="M140 156 C156 158 170 156 182 148"
                stroke="#fff"
                strokeOpacity="0.45"
                strokeWidth="2"
                fill="none"
                strokeLinecap="round"
              />
              <path
                d="M70 188 C64 162 72 136 100 134 C128 136 136 162 130 188 Z"
                fill={ref("skin")}
                stroke="#2b1250"
                strokeWidth="3"
                strokeLinejoin="round"
              />
              <ellipse cx="100" cy="166" rx="19" ry="18" fill={ref("belly")} />
              <ellipse
                cx="82"
                cy="188"
                rx="14"
                ry="6.5"
                fill={ref("skin")}
                stroke="#2b1250"
                strokeWidth="3"
              />
              <ellipse
                cx="118"
                cy="188"
                rx="14"
                ry="6.5"
                fill={ref("skin")}
                stroke="#2b1250"
                strokeWidth="3"
              />
              <g className="aaron__gills">
                <g>
                  <g transform="translate(46 86) rotate(-150)">
                    <g className="aaron__eq" style={{ animationDelay: "0.00s" }}>
                      <g fill="#2b1250" transform="scale(1.1)">
                        {GILL_1}
                      </g>
                      <g fill={ref("gill")}>{GILL_1}</g>
                      <path
                        d="M4 0 H28"
                        stroke="#fff"
                        strokeOpacity="0.45"
                        strokeWidth="2"
                        strokeLinecap="round"
                      />
                    </g>
                  </g>
                  <g transform="translate(42 100) rotate(-178)">
                    <g className="aaron__eq" style={{ animationDelay: "0.13s" }}>
                      <g fill="#2b1250" transform="scale(1.1)">
                        {GILL_2}
                      </g>
                      <g fill={ref("gill")}>{GILL_2}</g>
                      <path
                        d="M4 0 H30"
                        stroke="#fff"
                        strokeOpacity="0.45"
                        strokeWidth="2"
                        strokeLinecap="round"
                      />
                    </g>
                  </g>
                  <g transform="translate(46 114) rotate(154)">
                    <g className="aaron__eq" style={{ animationDelay: "0.26s" }}>
                      <g fill="#2b1250" transform="scale(1.1)">
                        {GILL_3}
                      </g>
                      <g fill={ref("gill")}>{GILL_3}</g>
                      <path
                        d="M4 0 H24"
                        stroke="#fff"
                        strokeOpacity="0.45"
                        strokeWidth="2"
                        strokeLinecap="round"
                      />
                    </g>
                  </g>
                </g>
                <g transform="translate(200 0) scale(-1 1)">
                  <g transform="translate(46 86) rotate(-150)">
                    <g className="aaron__eq" style={{ animationDelay: "0.39s" }}>
                      <g fill="#2b1250" transform="scale(1.1)">
                        {GILL_1}
                      </g>
                      <g fill={ref("gill")}>{GILL_1}</g>
                      <path
                        d="M4 0 H28"
                        stroke="#fff"
                        strokeOpacity="0.45"
                        strokeWidth="2"
                        strokeLinecap="round"
                      />
                    </g>
                  </g>
                  <g transform="translate(42 100) rotate(-178)">
                    <g className="aaron__eq" style={{ animationDelay: "0.52s" }}>
                      <g fill="#2b1250" transform="scale(1.1)">
                        {GILL_2}
                      </g>
                      <g fill={ref("gill")}>{GILL_2}</g>
                      <path
                        d="M4 0 H30"
                        stroke="#fff"
                        strokeOpacity="0.45"
                        strokeWidth="2"
                        strokeLinecap="round"
                      />
                    </g>
                  </g>
                  <g transform="translate(46 114) rotate(154)">
                    <g className="aaron__eq" style={{ animationDelay: "0.65s" }}>
                      <g fill="#2b1250" transform="scale(1.1)">
                        {GILL_3}
                      </g>
                      <g fill={ref("gill")}>{GILL_3}</g>
                      <path
                        d="M4 0 H24"
                        stroke="#fff"
                        strokeOpacity="0.45"
                        strokeWidth="2"
                        strokeLinecap="round"
                      />
                    </g>
                  </g>
                </g>
              </g>
              <g className="aaron__arm-rest">
                <g className="aaron__arm-sway" style={{ transformOrigin: "79px 141px" }}>
                  <g
                    fill="#2b1250"
                    stroke="#2b1250"
                    strokeWidth="6"
                    strokeLinejoin="round"
                    mask={ref("arml-fade")}
                  >
                    {REST_LEFT}
                  </g>
                  <clipPath id={id("arml-clip")}>{REST_LEFT}</clipPath>
                  <g fill={ref("skin")}>{REST_LEFT}</g>
                  <g clipPath={ref("arml-clip")} mask={ref("arml-fade")}>
                    <g fill="#7b3fc4" opacity="0.38">
                      {REST_LEFT}
                    </g>
                    <g fill={ref("skin")} transform="translate(-2.2 -2.6)">
                      {REST_LEFT}
                    </g>
                  </g>
                  <path
                    d="M69.87 145.25 L69.19 146.58 L68.6 147.91 L68.08 149.25 L67.65 150.6 L67.29 151.95 L67.03 153.3 L66.84 154.65 L66.74 155.99 L66.73 157.33"
                    stroke="#fff"
                    strokeOpacity="0.5"
                    strokeWidth="1.7"
                    fill="none"
                    strokeLinecap="round"
                  />
                  <ellipse cx="70.39" cy="164.68" rx="1.8" ry="1.1" fill="#fff" opacity="0.55" />
                </g>
              </g>
              <g className="aaron__arm-rest-right">
                <g
                  className="aaron__arm-sway aaron__arm-sway--right"
                  style={{ transformOrigin: "121px 141px" }}
                >
                  <g
                    fill="#2b1250"
                    stroke="#2b1250"
                    strokeWidth="6"
                    strokeLinejoin="round"
                    mask={ref("armr-fade")}
                  >
                    {REST_RIGHT}
                  </g>
                  <clipPath id={id("armr-clip")}>{REST_RIGHT}</clipPath>
                  <g fill={ref("skin")}>{REST_RIGHT}</g>
                  <g clipPath={ref("armr-clip")} mask={ref("armr-fade")}>
                    <g fill="#7b3fc4" opacity="0.38">
                      {REST_RIGHT}
                    </g>
                    <g fill={ref("skin")} transform="translate(-2.2 -2.6)">
                      {REST_RIGHT}
                    </g>
                  </g>
                  <path
                    d="M122.55 149.95 L123.16 150.82 L123.72 151.69 L124.21 152.55 L124.65 153.4 L125.02 154.25 L125.35 155.1 L125.61 155.95 L125.82 156.81 L125.98 157.67"
                    stroke="#fff"
                    strokeOpacity="0.5"
                    strokeWidth="1.7"
                    fill="none"
                    strokeLinecap="round"
                  />
                  <ellipse cx="129.61" cy="164.68" rx="1.8" ry="1.1" fill="#fff" opacity="0.55" />
                </g>
              </g>
              <path
                d="M100 54 C142 54 166 74 166 100 C166 126 138 142 100 142 C62 142 34 126 34 100 C34 74 58 54 100 54 Z"
                fill={ref("skin")}
                stroke="#2b1250"
                strokeWidth="3"
              />
              <path
                d="M58 70 C70 60 86 57 100 57"
                stroke="#fff"
                strokeOpacity="0.65"
                strokeWidth="3.5"
                fill="none"
                strokeLinecap="round"
              />
              <path
                d="M152 82 C160 92 162 104 158 114"
                stroke="#c9b6ff"
                strokeWidth="2.5"
                fill="none"
                strokeLinecap="round"
              />
              <g fill="#d96aa8" opacity="0.45">
                <circle cx="92" cy="68" r="2.4" />
                <circle cx="102" cy="64" r="1.8" />
                <circle cx="110" cy="70" r="2.2" />
                <circle cx="84" cy="74" r="1.5" />
              </g>
              <g className="aaron__face" ref={faceRef}>
                <g>
                  <clipPath id={id("ec0")}>
                    <ellipse cx="74" cy="98" rx="13.7" ry="14.2" />
                  </clipPath>
                  <ellipse cx="74" cy="98" rx="13" ry="13.5" fill={ref("white")} />
                  <g clipPath={ref("ec0")}>
                    <g className="aaron__pupil" ref={pupilLeftRef}>
                      <circle cx="74" cy="98" r="9" fill={ref("iris")} />
                      <circle cx="74" cy="98.54" r="4.68" fill="#120a2b" />
                      <circle cx="70.76" cy="94.4" r="3.06" fill="#fff" />
                      <circle cx="77.42" cy="101.24" r="1.35" fill="#fff" opacity="0.85" />
                    </g>
                    <ellipse cx="74" cy="98" rx="13" ry="13.5" fill={ref("eyeshade")} />
                    <g className="aaron__lid">
                      <path d="M59 82.5 H89 V104.08 Q74 116.22 59 104.08 Z" fill={ref("skin")} />
                      <path
                        d="M62.04 106.37 Q74 115.28 85.96 106.37"
                        fill="none"
                        stroke="#2b1250"
                        strokeWidth="2.6"
                        strokeLinecap="round"
                      />
                    </g>
                  </g>
                </g>

                <g>
                  <clipPath id={id("ec1")}>
                    <ellipse cx="126" cy="98" rx="13.7" ry="14.2" />
                  </clipPath>
                  <ellipse cx="126" cy="98" rx="13" ry="13.5" fill={ref("white")} />
                  <g clipPath={ref("ec1")}>
                    <g className="aaron__pupil" ref={pupilRightRef}>
                      <circle cx="126" cy="98" r="9" fill={ref("iris")} />
                      <circle cx="126" cy="98.54" r="4.68" fill="#120a2b" />
                      <circle cx="122.76" cy="94.4" r="3.06" fill="#fff" />
                      <circle cx="129.42" cy="101.24" r="1.35" fill="#fff" opacity="0.85" />
                    </g>
                    <ellipse cx="126" cy="98" rx="13" ry="13.5" fill={ref("eyeshade")} />
                    <g className="aaron__lid">
                      <path
                        d="M111 82.5 H141 V104.08 Q126 116.22 111 104.08 Z"
                        fill={ref("skin")}
                      />
                      <path
                        d="M114.04 106.37 Q126 115.28 137.96 106.37"
                        fill="none"
                        stroke="#2b1250"
                        strokeWidth="2.6"
                        strokeLinecap="round"
                      />
                    </g>
                  </g>
                </g>
                <ellipse cx="56" cy="116" rx="9" ry="5" fill="#ff5f9e" opacity="0.4" />
                <ellipse cx="144" cy="116" rx="9" ry="5" fill="#ff5f9e" opacity="0.4" />
                <g>
                  <path
                    className="aaron__mouth-idle"
                    d="M84 117 Q100 129 116 117"
                    stroke="#2b1250"
                    strokeWidth="2.6"
                    fill="none"
                    strokeLinecap="round"
                  />
                  <g className="aaron__mouth-talk">
                    <path
                      d="M84 116 Q100 114 116 116 Q112 134 100 135 Q88 134 84 116 Z"
                      fill="#5c1840"
                      stroke="#2b1250"
                      strokeWidth="2.4"
                      strokeLinejoin="round"
                    />
                    <ellipse cx="100" cy="130" rx="7" ry="3.6" fill="#ff7fa3" />
                  </g>
                </g>
              </g>
              <g className="aaron__rings" fill="none" stroke="#ffd0e6" strokeWidth="2">
                <circle
                  className="aaron__spark"
                  cx="30"
                  cy="66"
                  r="4"
                  style={{ animationDelay: ".2s" }}
                />
                <circle
                  className="aaron__spark"
                  cx="170"
                  cy="60"
                  r="3.2"
                  style={{ animationDelay: ".7s" }}
                />
                <circle
                  className="aaron__spark"
                  cx="160"
                  cy="40"
                  r="2.4"
                  style={{ animationDelay: "1.1s" }}
                />
              </g>
              <g className="aaron__arm-point" style={arms.left}>
                <g className="aaron__arm-pop" style={{ transformOrigin: "78px 146.5px" }}>
                  <g
                    fill="#2b1250"
                    stroke="#2b1250"
                    strokeWidth="6"
                    strokeLinejoin="round"
                    mask={ref("pul-fade")}
                  >
                    {UPPER_LEFT}
                  </g>
                  <g className="aaron__forearm" style={forearms.left}>
                    <g fill="#2b1250" stroke="#2b1250" strokeWidth="6" strokeLinejoin="round">
                      {FORE_LEFT}
                    </g>
                  </g>
                  <clipPath id={id("pul-clip")}>{UPPER_LEFT}</clipPath>
                  <g fill={ref("skin")}>{UPPER_LEFT}</g>
                  <g clipPath={ref("pul-clip")} mask={ref("pul-fade")}>
                    <g fill="#7b3fc4" opacity="0.38">
                      {UPPER_LEFT}
                    </g>
                    <g fill={ref("skin")} transform="translate(-2.2 -2.6)">
                      {UPPER_LEFT}
                    </g>
                  </g>
                  <path
                    d="M69.78 141.6 L68.85 141.59 L67.92 141.57 L66.99 141.54 L66.06 141.51 L65.13 141.47 L64.2 141.43 L63.27 141.38 L62.33 141.33 L61.4 141.27"
                    stroke="#fff"
                    strokeOpacity="0.5"
                    strokeWidth="1.7"
                    fill="none"
                    strokeLinecap="round"
                  />
                  <g className="aaron__forearm" style={forearms.left}>
                    <clipPath id={id("pfl-clip")}>{FORE_LEFT}</clipPath>
                    <g fill={ref("skin")}>{FORE_LEFT}</g>
                    <g clipPath={ref("pfl-clip")}>
                      <g fill="#7b3fc4" opacity="0.38">
                        {FORE_LEFT}
                      </g>
                      <g fill={ref("skin")} transform="translate(-2.2 -2.6)">
                        {FORE_LEFT}
                      </g>
                    </g>
                    <path
                      d="M58.03 141.01 L57.29 140.95 L56.54 140.88 L55.8 140.81 L55.05 140.74 L54.31 140.66 L53.57 140.58 L52.82 140.51 L52.08 140.42 L51.33 140.34 L50.58 140.25 L49.84 140.16 L49.09 140.07"
                      stroke="#fff"
                      strokeOpacity="0.5"
                      strokeWidth="1.7"
                      fill="none"
                      strokeLinecap="round"
                    />
                  </g>
                </g>
              </g>
              <g className="aaron__arm-point aaron__arm-point--right" style={arms.right}>
                <g className="aaron__arm-pop" style={{ transformOrigin: "122px 146.5px" }}>
                  <g
                    fill="#2b1250"
                    stroke="#2b1250"
                    strokeWidth="6"
                    strokeLinejoin="round"
                    mask={ref("pur-fade")}
                  >
                    {UPPER_RIGHT}
                  </g>
                  <g className="aaron__forearm" style={forearms.right}>
                    <g fill="#2b1250" stroke="#2b1250" strokeWidth="6" strokeLinejoin="round">
                      {FORE_RIGHT}
                    </g>
                  </g>
                  <clipPath id={id("pur-clip")}>{UPPER_RIGHT}</clipPath>
                  <g fill={ref("skin")}>{UPPER_RIGHT}</g>
                  <g clipPath={ref("pur-clip")} mask={ref("pur-fade")}>
                    <g fill="#7b3fc4" opacity="0.38">
                      {UPPER_RIGHT}
                    </g>
                    <g fill={ref("skin")} transform="translate(-2.2 -2.6)">
                      {UPPER_RIGHT}
                    </g>
                  </g>
                  <path
                    d="M130.22 141.6 L131.15 141.59 L132.08 141.57 L133.01 141.54 L133.94 141.51 L134.87 141.47 L135.8 141.43 L136.73 141.38 L137.67 141.33 L138.6 141.27"
                    stroke="#fff"
                    strokeOpacity="0.5"
                    strokeWidth="1.7"
                    fill="none"
                    strokeLinecap="round"
                  />
                  <g className="aaron__forearm" style={forearms.right}>
                    <clipPath id={id("pfr-clip")}>{FORE_RIGHT}</clipPath>
                    <g fill={ref("skin")}>{FORE_RIGHT}</g>
                    <g clipPath={ref("pfr-clip")}>
                      <g fill="#7b3fc4" opacity="0.38">
                        {FORE_RIGHT}
                      </g>
                      <g fill={ref("skin")} transform="translate(-2.2 -2.6)">
                        {FORE_RIGHT}
                      </g>
                    </g>
                    <path
                      d="M141.97 141.01 L142.71 140.95 L143.46 140.88 L144.2 140.81 L144.95 140.74 L145.69 140.66 L146.43 140.58 L147.18 140.51 L147.92 140.42 L148.67 140.34 L149.42 140.25 L150.16 140.16 L150.91 140.07"
                      stroke="#fff"
                      strokeOpacity="0.5"
                      strokeWidth="1.7"
                      fill="none"
                      strokeLinecap="round"
                    />
                  </g>
                </g>
              </g>
            </g>
          </g>
        </g>
      </g>
    </svg>
  );
}
