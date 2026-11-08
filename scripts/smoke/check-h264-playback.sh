#!/usr/bin/env bash
# Decodes an H.264 + AAC sample with the gst-libav plugin a Linux package of Mumcord bundles
# (scripts/build-ffmpeg.sh), the way the webview plays Twitch and .mp4 videos. The caller sets up
# the GStreamer environment of the package (plugin path, registry, for the AppImage also its
# libraries and scanner). Fails when avdec_h264 or avdec_aac is missing, comes from another
# plugin file than <plugin>, or cannot decode the sample.
#
#   scripts/smoke/check-h264-playback.sh <plugin> [sample]
#
# The default sample, scripts/smoke/fixtures/h264-aac.mp4, is two seconds of FFmpeg's synthetic
# test picture and a sine tone (no third-party content), made with:
#   ffmpeg -f lavfi -i testsrc2=size=160x120:rate=15:duration=2 \
#     -f lavfi -i sine=frequency=440:sample_rate=44100:duration=2 \
#     -c:v libx264 -profile:v baseline -pix_fmt yuv420p -preset veryslow -crf 35 \
#     -c:a aac -b:a 32k -ac 1 -map_metadata -1 -fflags +bitexact -movflags +faststart h264-aac.mp4
set -euo pipefail

plugin=${1:?usage: check-h264-playback.sh <plugin> [sample]}
sample=${2:-$(dirname "$0")/fixtures/h264-aac.mp4}

for element in avdec_h264 avdec_aac; do
  file=$(gst-inspect-1.0 "$element" | awk '$1 == "Filename" { print $2 }')
  if [[ "$file" != "$plugin" ]]; then
    echo "$element comes from '${file:-nowhere}', not from $plugin" >&2
    exit 1
  fi
done

# h264parse and aacparse sit in front of the decoders as in WebKit's pipelines; fakesink with
# sync=false decodes as fast as possible. Any decoding or negotiation error fails gst-launch.
gst-launch-1.0 -q filesrc location="$sample" ! qtdemux name=demux \
  demux.video_0 ! queue ! h264parse ! avdec_h264 ! fakesink sync=false \
  demux.audio_0 ! queue ! aacparse ! avdec_aac ! fakesink sync=false
echo "decoded $sample with $plugin"
