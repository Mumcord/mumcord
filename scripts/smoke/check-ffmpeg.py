"""Checks the FFmpeg that a Linux package of Mumcord bundles (scripts/build-ffmpeg.sh).

Usage: python3 -I scripts/smoke/check-ffmpeg.py <libavcodec>

Reads the build configuration (what `ffmpeg -buildconf` prints) and the licence from the bundled
libavcodec itself and lists the codecs it contains: the build must be LGPL only (no GPL, no
nonfree, no version 3), contain no encoders and decode exactly H.264 and AAC.
"""

import ctypes
import sys

EXPECTED_DECODERS = {"h264", "aac", "aac_latm"}
FORBIDDEN_FLAGS = ("--enable-gpl", "--enable-nonfree", "--enable-version3")


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__.strip().splitlines()[2], file=sys.stderr)
        return 2
    lib = ctypes.CDLL(sys.argv[1])
    lib.avcodec_configuration.restype = ctypes.c_char_p
    lib.avcodec_license.restype = ctypes.c_char_p
    lib.av_codec_iterate.restype = ctypes.c_void_p
    lib.av_codec_iterate.argtypes = [ctypes.POINTER(ctypes.c_void_p)]
    lib.av_codec_is_encoder.argtypes = [ctypes.c_void_p]
    lib.av_codec_is_decoder.argtypes = [ctypes.c_void_p]

    configuration = lib.avcodec_configuration().decode()
    licence = lib.avcodec_license().decode()
    print(f"configuration: {configuration}")
    print(f"license: {licence}")

    problems = [f"configured with {flag}" for flag in FORBIDDEN_FLAGS if flag in configuration]
    if licence != "LGPL version 2.1 or later":
        problems.append(f"licence is {licence!r}")

    decoders, encoders = set(), set()
    opaque = ctypes.c_void_p()
    while codec := lib.av_codec_iterate(ctypes.byref(opaque)):
        # AVCodec starts with `const char *name`.
        name = ctypes.cast(codec, ctypes.POINTER(ctypes.c_char_p))[0].decode()
        if lib.av_codec_is_encoder(codec):
            encoders.add(name)
        if lib.av_codec_is_decoder(codec):
            decoders.add(name)
    print(f"decoders: {', '.join(sorted(decoders))}")
    if encoders:
        problems.append(f"contains encoders: {', '.join(sorted(encoders))}")
    if decoders != EXPECTED_DECODERS:
        problems.append(f"decoders are not {', '.join(sorted(EXPECTED_DECODERS))}")

    for problem in problems:
        print(f"bundled FFmpeg: {problem}", file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
