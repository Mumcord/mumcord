"""Checks the ONNX Runtime library an installed Mumcord package carries (spec 08, FR-8.33).

Usage: python3 scripts/smoke/check-onnxruntime.py <library>

<library> is the file at the place crates/effects/src/runtime.rs looks for it in the installed
package. The check loads it the way the `ort` crate does (OrtGetApiBase, then GetApi with the
API version Mumcord asks for) and checks the version, so a missing, broken, wrong-architecture or
blocked library fails here instead of showing "background effects are not available" to users.
"""

import ctypes
import sys

EXPECTED_VERSION = "1.23.2"
# ort 2.0.0-rc.13 without api-* features asks for API version 17 (ort-sys src/version.rs).
API_VERSION = 17


class OrtApiBase(ctypes.Structure):
    _fields_ = [
        ("GetApi", ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_uint32)),
        ("GetVersionString", ctypes.CFUNCTYPE(ctypes.c_char_p)),
    ]


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__.strip().splitlines()[2], file=sys.stderr)
        return 2
    path = sys.argv[1]
    try:
        library = ctypes.CDLL(path)
    except OSError as err:
        print(f"FAIL: cannot load ONNX Runtime from {path}: {err}", file=sys.stderr)
        return 1
    library.OrtGetApiBase.restype = ctypes.POINTER(OrtApiBase)
    base = library.OrtGetApiBase().contents
    version = base.GetVersionString().decode()
    if version != EXPECTED_VERSION:
        print(f"FAIL: {path} is ONNX Runtime {version}, expected {EXPECTED_VERSION}", file=sys.stderr)
        return 1
    if not base.GetApi(API_VERSION):
        print(f"FAIL: ONNX Runtime {version} does not offer API version {API_VERSION}", file=sys.stderr)
        return 1
    print(f"ONNX Runtime {version} loads from {path}: background effects available")
    return 0


if __name__ == "__main__":
    sys.exit(main())
