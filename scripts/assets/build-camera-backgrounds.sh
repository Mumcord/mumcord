#!/usr/bin/env bash
# Builds Mumcord's 18 built-in camera background images (spec 08, FR-8.39) from the
# original artwork. The originals (1672x941 JPEGs, made for Mumcord, our own work) are not
# kept in the repository; only the converted files in assets/backgrounds/ are.
#
# Each image is scaled to cover 1280x720, center-cropped to 16:9, stripped of metadata and
# written as WebP. The quality starts at 85 and drops in steps of 5 until the file is at most
# 300 KB, the size limit of FR-8.39.
#
# The file name (without .webp) is the image id: the stored setting value `builtin:{id}` and
# the name the app loads from its resource folder `camera-backgrounds/`. When adding an
# image, also list it in src-tauri/tauri.conf.json, BUILTIN_BACKGROUNDS
# (ui/src/features/camera/preview/previewLogic.ts), the mock list (ui/src/api/mockCamera.ts)
# and the `backgrounds` texts in ui/src/locales/*/camera.json.
#
# Needs ImageMagick 7 with WebP support and Python 3 (to match file names whether their
# umlauts are stored composed or decomposed, as archives made on macOS do).
# Usage: scripts/assets/build-camera-backgrounds.sh <folder with the original JPEGs>
set -euo pipefail

src=${1:?usage: $0 <folder with the original JPEGs>}
out="$(cd "$(dirname "$0")/../.." && pwd)/assets/backgrounds"
max_bytes=$((300 * 1024))

# original file name | id
images=(
  "Abstrakte Lichtwellen mit warmer Sonnenglut.jpg|abstract-warm-waves"
  "Futuristische Raumschiffbrücke mit schlafender Katze.jpg|spaceship-bridge-cat"
  "Futuristische Raumstation mit Erde im Blick.jpg|space-station-earth"
  "Futuristischer Serverraum-Korridor.jpg|server-corridor"
  "Gemütliche Modelleisenbahn bei Nacht.jpg|night-model-railway"
  "Glossige Wellen mit leuchtender Kugel.jpg|glossy-waves-orb"
  "Hochmodernes Elektronik-Maker-Labor.jpg|electronics-lab"
  "Leuchtende Abstrakte Wellen in Blau und Magenta.jpg|blue-magenta-waves"
  "Luxuriöser Balkon mit Stadtblick bei Nacht.jpg|city-balcony-night"
  "Modernes Neon-Riffaquarium im Hobbyraum.jpg|reef-aquarium-room"
  "Modernes WC mit warmem Ambientelicht.jpg|modern-wc"
  "Nächtliches Modellbahn-Diorama mit Viadukt.jpg|model-railway-viaduct"
  "Neon-Gamingzimmer mit Stadtblick.jpg|neon-gaming-room"
  "Neon-Nerdzimmer mit gemütlicher Gaming-Ecke.jpg|neon-nerd-room"
  "Neon-Pixel-Gamingzimmer mit schlafender Katze.jpg|pixel-gaming-room-cat"
  "Neon-Sonnenuntergang über Pixelbergen.jpg|pixel-mountain-sunset"
  "Sci-Fi-Zimmer mit goldener Talansicht.jpg|scifi-room-golden-valley"
  "Unterwasserbasis mit Wal und Hightech-Arbeitsplatz.jpg|underwater-base-whale"
)

# Prints the path of the file in $src whose name matches $1 after Unicode NFC normalisation.
find_original() {
  python3 -I -c 'import os, sys, unicodedata as u
want = u.normalize("NFC", sys.argv[2])
hits = [n for n in os.listdir(sys.argv[1]) if u.normalize("NFC", n) == want]
if not hits: sys.exit(f"missing original: {want}")
print(os.path.join(sys.argv[1], hits[0]))' "$src" "$1"
}

mkdir -p "$out"
for entry in "${images[@]}"; do
  name=${entry%|*}
  id=${entry#*|}
  original=$(find_original "$name")
  target="$out/$id.webp"
  quality=85
  while :; do
    magick "$original" -resize 1280x720^ -gravity center -extent 1280x720 -strip \
      -quality "$quality" -define webp:method=6 "$target"
    size=$(stat -c %s "$target")
    if ((size <= max_bytes)); then break; fi
    if ((quality <= 30)); then
      echo "$id: still $size bytes at quality $quality" >&2
      exit 1
    fi
    quality=$((quality - 5))
  done
  echo "$id.webp: $size bytes (quality $quality)"
done
