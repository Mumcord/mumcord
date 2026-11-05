#!/usr/bin/env python3
"""Draws Mumcord's six built-in camera background images (spec 08, FR-8.39).

Each scene is written as an SVG from simple shapes with a fixed random seed, rendered with
rsvg-convert and finished with ImageMagick (a slight softening so the scene reads as an
out-of-focus room behind the person, light grain against colour banding, metadata stripped).
The output is a 1280x720 JPEG of at most 300 KB per image. The images are Mumcord's own work
and share the project's licence; no third-party artwork, photo or font is used.

Needs Python 3, rsvg-convert (librsvg) and ImageMagick 7.
Run: python3 generate.py   (writes {id}.jpg next to this script)

The ids are the stored setting values (`builtin:{id}`) and the file names the app loads from
its resource folder `camera-backgrounds/`; the names are the ones the preview shows.
"""
import math
import os
import random
import subprocess
import tempfile

W, H = 1280, 720
MAX_BYTES = 300 * 1024
HERE = os.path.dirname(os.path.abspath(__file__))


def svg(body, defs=""):
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" '
        f'viewBox="0 0 {W} {H}"><defs>{defs}</defs>{body}</svg>'
    )


def blur_filter(fid, std):
    return (
        f'<filter id="{fid}" x="-50%" y="-50%" width="200%" height="200%">'
        f'<feGaussianBlur stdDeviation="{std}"/></filter>'
    )


def vignette(strength=0.35):
    defs = (
        '<radialGradient id="vig" cx="0.5" cy="0.5" r="0.75">'
        '<stop offset="0.55" stop-color="#000" stop-opacity="0"/>'
        f'<stop offset="1" stop-color="#000" stop-opacity="{strength}"/></radialGradient>'
    )
    return defs, f'<rect width="{W}" height="{H}" fill="url(#vig)"/>'


def ridge(rng, base, amp, roughness, steps=7):
    """A mountain ridge line by midpoint displacement, as a closed path down to the bottom."""
    pts = [base + rng.uniform(-amp, amp), base + rng.uniform(-amp, amp)]
    spread = amp
    for _ in range(steps):
        nxt = []
        for a, b in zip(pts, pts[1:]):
            nxt += [a, (a + b) / 2 + rng.uniform(-spread, spread)]
        pts = nxt + [pts[-1]]
        spread *= roughness
    step = W / (len(pts) - 1)
    line = " ".join(f"L{i * step:.1f},{y:.1f}" for i, y in enumerate(pts))
    return f"M0,{H} {line} L{W},{H} Z"


def soft_light():
    rng = random.Random(1)
    defs = (
        '<linearGradient id="g" x1="0" y1="0" x2="1" y2="1">'
        '<stop offset="0" stop-color="#8f7cff"/><stop offset="0.55" stop-color="#e58fd0"/>'
        '<stop offset="1" stop-color="#ffc59e"/></linearGradient>'
        + blur_filter("b1", 60)
        + blur_filter("b2", 14)
    )
    body = f'<rect width="{W}" height="{H}" fill="url(#g)"/>'
    body += '<g filter="url(#b1)">'
    body += '<circle cx="240" cy="160" r="210" fill="#fff" opacity="0.22"/>'
    body += '<circle cx="1040" cy="560" r="270" fill="#fff" opacity="0.18"/>'
    body += '<circle cx="1100" cy="120" r="140" fill="#ffd9f0" opacity="0.25"/>'
    body += "</g>"
    # Bokeh dots, kept away from the middle where the person sits.
    body += '<g filter="url(#b2)">'
    for _ in range(22):
        x = rng.choice([rng.uniform(40, 400), rng.uniform(880, 1240)])
        y = rng.uniform(40, 680)
        r = rng.uniform(14, 46)
        body += f'<circle cx="{x:.0f}" cy="{y:.0f}" r="{r:.0f}" fill="#fff" opacity="{rng.uniform(0.12, 0.3):.2f}"/>'
    body += "</g>"
    return svg(body, defs), 0.0


def library():
    rng = random.Random(2)
    colours = ["#8c3a32", "#35577a", "#a8803a", "#36604f", "#5e3a5e", "#9a5a2c", "#2f7a62", "#7a4a2a", "#b49a6a", "#4a4f6a"]
    vd, vb = vignette(0.45)
    defs = (
        vd
        + '<radialGradient id="lamp" cx="0.18" cy="0.12" r="0.8">'
        '<stop offset="0" stop-color="#ffd9a0" stop-opacity="0.35"/>'
        '<stop offset="1" stop-color="#ffd9a0" stop-opacity="0"/></radialGradient>'
        '<linearGradient id="spine" x1="0" y1="0" x2="1" y2="0">'
        '<stop offset="0" stop-color="#000" stop-opacity="0.25"/>'
        '<stop offset="0.3" stop-color="#fff" stop-opacity="0.12"/>'
        '<stop offset="1" stop-color="#000" stop-opacity="0.3"/></linearGradient>'
    )
    body = f'<rect width="{W}" height="{H}" fill="#4a3424"/>'
    shelf_tops = [40, 270, 500]
    shelf_h = 200
    for top in shelf_tops:
        body += f'<rect x="0" y="{top}" width="{W}" height="{shelf_h}" fill="#3a281b"/>'
        x = 24.0
        while x < W - 24:
            if rng.random() < 0.06:
                x += rng.uniform(30, 70)  # a gap on the shelf
                continue
            w = rng.uniform(22, 44)
            h = rng.uniform(shelf_h * 0.68, shelf_h * 0.92)
            colour = rng.choice(colours)
            y = top + shelf_h - h
            if rng.random() < 0.05 and x + h < W - 24:
                # A book lying flat.
                body += f'<rect x="{x:.1f}" y="{top + shelf_h - w:.1f}" width="{h:.1f}" height="{w:.1f}" fill="{colour}"/>'
                x += h + 4
                continue
            body += f'<rect x="{x:.1f}" y="{y:.1f}" width="{w:.1f}" height="{h:.1f}" fill="{colour}"/>'
            body += f'<rect x="{x:.1f}" y="{y:.1f}" width="{w:.1f}" height="{h:.1f}" fill="url(#spine)"/>'
            band = y + h * rng.uniform(0.12, 0.2)
            body += f'<rect x="{x + 2:.1f}" y="{band:.1f}" width="{w - 4:.1f}" height="5" fill="#e8d3a0" opacity="0.45"/>'
            x += w + rng.uniform(1, 3)
        body += f'<rect x="0" y="{top + shelf_h}" width="{W}" height="30" fill="#2e1f14"/>'
        body += f'<rect x="0" y="{top + shelf_h}" width="{W}" height="4" fill="#6a4a32" opacity="0.6"/>'
    body += f'<rect width="{W}" height="{H}" fill="url(#lamp)"/>' + vb
    return svg(body, defs), 2.2


def mountains():
    rng = random.Random(3)
    defs = (
        '<linearGradient id="sky" x1="0" y1="0" x2="0" y2="1">'
        '<stop offset="0" stop-color="#7fb2e5"/><stop offset="1" stop-color="#dcebf7"/></linearGradient>'
        '<linearGradient id="snow" x1="0" y1="0" x2="0" y2="1">'
        '<stop offset="0" stop-color="#ffffff"/><stop offset="1" stop-color="#ffffff" stop-opacity="0"/></linearGradient>'
        + blur_filter("cloud", 18)
    )
    body = f'<rect width="{W}" height="{H}" fill="url(#sky)"/>'
    body += '<g filter="url(#cloud)" fill="#fff" opacity="0.7">'
    for cx, cy in [(220, 120), (980, 90), (1150, 170)]:
        for _ in range(5):
            body += f'<ellipse cx="{cx + rng.uniform(-70, 70):.0f}" cy="{cy + rng.uniform(-12, 12):.0f}" rx="{rng.uniform(50, 90):.0f}" ry="{rng.uniform(16, 26):.0f}"/>'
    body += "</g>"
    far = ridge(rng, 300, 170, 0.6, steps=6)
    body += f'<path d="{far}" fill="#8fa6c2"/>'
    body += f'<path d="{far}" fill="url(#snow)" opacity="0.55"/>'
    body += f'<path d="{ridge(rng, 420, 90, 0.58, steps=6)}" fill="#6f86a3"/>'
    body += f'<path d="{ridge(rng, 520, 40, 0.5)}" fill="#5f8a6f"/>'
    body += f'<path d="{ridge(rng, 600, 26, 0.5)}" fill="#4f7a5a"/>'
    body += f'<path d="{ridge(rng, 660, 18, 0.5)}" fill="#3f6a4a"/>'
    return svg(body, defs), 1.2


def studio():
    vd, vb = vignette(0.25)
    defs = (
        vd
        + '<linearGradient id="wall" x1="0" y1="0" x2="1" y2="0">'
        '<stop offset="0" stop-color="#f3eee6"/><stop offset="1" stop-color="#e2dacd"/></linearGradient>'
        '<linearGradient id="floor" x1="0" y1="0" x2="0" y2="1">'
        '<stop offset="0" stop-color="#c9b79c"/><stop offset="1" stop-color="#b49f80"/></linearGradient>'
        '<linearGradient id="sun" x1="0" y1="0" x2="1" y2="1">'
        '<stop offset="0" stop-color="#fff" stop-opacity="0.25"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></linearGradient>'
        '<linearGradient id="art" x1="0" y1="0" x2="1" y2="1">'
        '<stop offset="0" stop-color="#f0c35e"/><stop offset="1" stop-color="#d9893c"/></linearGradient>'
        + blur_filter("shadow", 10)
        + blur_filter("leaf", 1.5)
    )
    body = f'<rect width="{W}" height="{H}" fill="url(#wall)"/>'
    body += f'<rect x="0" y="520" width="{W}" height="200" fill="url(#floor)"/>'
    body += f'<rect x="0" y="514" width="{W}" height="10" fill="#d8ccb8"/>'
    # Window light falling across the wall.
    body += '<path d="M520 0 L760 0 L560 520 L320 520 Z" fill="url(#sun)"/>'
    # Framed picture: an abstract warm print.
    body += '<rect x="138" y="128" width="300" height="225" fill="#000" opacity="0.18" filter="url(#shadow)" transform="translate(8 10)"/>'
    body += '<rect x="138" y="128" width="300" height="225" fill="#2d3e50"/>'
    body += '<rect x="158" y="148" width="260" height="185" fill="#f6f1e8"/>'
    body += '<rect x="176" y="166" width="224" height="149" fill="url(#art)"/>'
    body += '<circle cx="330" cy="215" r="34" fill="#f6f1e8" opacity="0.85"/>'
    body += '<path d="M176 315 L250 245 L300 285 L345 250 L400 300 L400 315 Z" fill="#2d3e50" opacity="0.75"/>'
    # Side table with a plant on the right.
    body += '<ellipse cx="1030" cy="640" rx="120" ry="16" fill="#000" opacity="0.15" filter="url(#shadow)"/>'
    body += '<rect x="960" y="470" width="140" height="16" rx="3" fill="#6b4f3a"/>'
    body += '<rect x="975" y="486" width="10" height="150" fill="#5a412f"/>'
    body += '<rect x="1075" y="486" width="10" height="150" fill="#5a412f"/>'
    body += '<path d="M995 470 L1003 410 L1057 410 L1065 470 Z" fill="#d9cbb5"/>'
    body += '<rect x="1026" y="300" width="8" height="112" fill="#6b4f3a"/>'
    body += '<g filter="url(#leaf)">'
    for cx, cy, r, c in [(1030, 250, 78, "#8fb07f"), (985, 285, 52, "#9fbf8f"), (1078, 280, 56, "#86a877"), (1030, 205, 50, "#a9c79a")]:
        body += f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="{c}"/>'
    body += "</g>"
    body += vb
    return svg(body, defs), 1.4


def night_city():
    rng = random.Random(5)
    defs = (
        '<linearGradient id="sky" x1="0" y1="0" x2="0" y2="1">'
        '<stop offset="0" stop-color="#0d1230"/><stop offset="1" stop-color="#2a2f5e"/></linearGradient>'
        '<radialGradient id="moon" cx="0.5" cy="0.5" r="0.5">'
        '<stop offset="0" stop-color="#f5f1d0" stop-opacity="0.5"/><stop offset="1" stop-color="#f5f1d0" stop-opacity="0"/></radialGradient>'
        + blur_filter("glow", 3)
    )
    body = f'<rect width="{W}" height="{H}" fill="url(#sky)"/>'
    for _ in range(90):
        body += f'<circle cx="{rng.uniform(0, W):.0f}" cy="{rng.uniform(0, 300):.0f}" r="{rng.uniform(0.8, 1.8):.1f}" fill="#fff" opacity="{rng.uniform(0.3, 0.9):.2f}"/>'
    body += '<circle cx="1040" cy="110" r="110" fill="url(#moon)"/>'
    body += '<circle cx="1040" cy="110" r="44" fill="#f5f1d0"/>'
    lit = []
    for layer, (lo, hi, fill, win_p) in enumerate([(160, 420, "#222a55", 0.25), (240, 480, "#1a2149", 0.35)]):
        x = -20.0
        while x < W:
            w = rng.uniform(70, 150)
            top = H - rng.uniform(lo, hi) * (0.75 if 420 < x < 860 else 1.0)
            body += f'<rect x="{x:.0f}" y="{top:.0f}" width="{w:.0f}" height="{H - top:.0f}" fill="{fill}"/>'
            if layer == 1:
                wy = top + 18
                while wy < H - 20:
                    wx = x + 12
                    while wx < x + w - 18:
                        if rng.random() < win_p:
                            lit.append(f'<rect x="{wx:.0f}" y="{wy:.0f}" width="8" height="12" fill="{rng.choice(["#ffd66b", "#ffe9a8", "#f4b860"])}" opacity="{rng.uniform(0.6, 1):.2f}"/>')
                        wx += 20
                    wy += 26
            x += w + rng.uniform(4, 14)
    body += '<g filter="url(#glow)" opacity="0.8">' + "".join(lit) + "</g>" + "".join(lit)
    return svg(body, defs), 0.9


def beach():
    rng = random.Random(6)
    defs = (
        '<linearGradient id="sky" x1="0" y1="0" x2="0" y2="1">'
        '<stop offset="0" stop-color="#7cc4ec"/><stop offset="1" stop-color="#d4eef9"/></linearGradient>'
        '<linearGradient id="sea" x1="0" y1="0" x2="0" y2="1">'
        '<stop offset="0" stop-color="#5bb3d6"/><stop offset="1" stop-color="#2f8fb8"/></linearGradient>'
        '<linearGradient id="sand" x1="0" y1="0" x2="0" y2="1">'
        '<stop offset="0" stop-color="#ecd29b"/><stop offset="1" stop-color="#f5e3bc"/></linearGradient>'
        '<radialGradient id="sun" cx="0.5" cy="0.5" r="0.5">'
        '<stop offset="0" stop-color="#fff8c8" stop-opacity="0.8"/><stop offset="1" stop-color="#fff8c8" stop-opacity="0"/></radialGradient>'
        + blur_filter("soft", 6)
    )
    body = f'<rect width="{W}" height="{H}" fill="url(#sky)"/>'
    body += '<circle cx="250" cy="140" r="150" fill="url(#sun)"/><circle cx="250" cy="140" r="48" fill="#fff6c4"/>'
    body += f'<rect y="370" width="{W}" height="140" fill="url(#sea)"/>'
    body += f'<rect y="368" width="{W}" height="4" fill="#e8f6fb" opacity="0.6"/>'
    for _ in range(14):
        y = rng.uniform(390, 495)
        x = rng.uniform(-100, W)
        body += f'<rect x="{x:.0f}" y="{y:.0f}" width="{rng.uniform(80, 260):.0f}" height="2" fill="#fff" opacity="0.35"/>'
    body += f'<path d="M0 500 Q320 485 640 498 T{W} 492 L{W} {H} L0 {H} Z" fill="url(#sand)"/>'
    body += f'<path d="M0 500 Q320 485 640 498 T{W} 492" stroke="#fff" stroke-width="6" fill="none" opacity="0.6" filter="url(#soft)"/>'
    # A palm tree at the right edge.
    body += '<path d="M1110 640 C1105 520 1120 400 1170 290" stroke="#7a5230" stroke-width="18" fill="none" stroke-linecap="round"/>'
    for angle, length in [(-160, 170), (-130, 150), (-95, 120), (-50, 150), (-15, 165), (20, 140), (175, 140)]:
        a = math.radians(angle)
        ex, ey = 1170 + length * math.cos(a), 290 + length * math.sin(a) + 40
        mx, my = 1170 + length * 0.5 * math.cos(a), 290 + length * 0.5 * math.sin(a) - 25
        body += f'<path d="M1170 290 Q{mx:.0f} {my:.0f} {ex:.0f} {ey:.0f}" stroke="#3f8f4f" stroke-width="22" fill="none" stroke-linecap="round"/>'
    body += '<ellipse cx="1120" cy="650" rx="90" ry="12" fill="#000" opacity="0.08" filter="url(#soft)"/>'
    return svg(body, defs), 1.0


# id → (name shown in the preview, drawing function); order as in the preview library.
SCENES = {
    "soft-light": ("Soft light", soft_light),
    "library": ("Library", library),
    "mountains": ("Mountains", mountains),
    "studio": ("Studio", studio),
    "night-city": ("Night city", night_city),
    "beach": ("Beach", beach),
}


def render(image_id, draw, work):
    source, softness = draw()
    svg_path = os.path.join(work, f"{image_id}.svg")
    png_path = os.path.join(work, f"{image_id}.png")
    with open(svg_path, "w") as f:
        f.write(source)
    subprocess.run(["rsvg-convert", "-w", str(W), "-h", str(H), svg_path, "-o", png_path], check=True)
    out = os.path.join(HERE, f"{image_id}.jpg")
    finish = ["-blur", f"0x{softness}"] if softness else []
    for quality in range(90, 59, -5):
        subprocess.run(
            ["magick", png_path, *finish, "-seed", "1", "-attenuate", "0.08", "+noise", "Gaussian",
             "-strip", "-sampling-factor", "4:2:0", "-interlace", "JPEG", "-quality", str(quality), out],
            check=True,
        )
        if os.path.getsize(out) <= MAX_BYTES:
            return quality, os.path.getsize(out)
    raise SystemExit(f"{image_id}.jpg stays above {MAX_BYTES} bytes")


def main():
    with tempfile.TemporaryDirectory() as work:
        for image_id, (name, draw) in SCENES.items():
            quality, size = render(image_id, draw, work)
            print(f"{image_id}.jpg ({name}): quality {quality}, {size // 1024} KB")


if __name__ == "__main__":
    main()
