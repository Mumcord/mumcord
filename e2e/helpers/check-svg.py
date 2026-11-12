#!/usr/bin/env python3
"""Load the native dialog's fallback icon through GTK's real image loader.

Run before WebDriver so a broken SVG loader reports its own diagnostics instead
of crashing the app in a file chooser and causing cascading session timeouts.
"""

import ctypes
import sys
from pathlib import Path


class GError(ctypes.Structure):
    _fields_ = [
        ("domain", ctypes.c_uint),
        ("code", ctypes.c_int),
        ("message", ctypes.c_char_p),
    ]


def main() -> int:
    icon = Path("/usr/share/icons/Adwaita/scalable/status/image-missing.svg")
    pixbuf = ctypes.CDLL("libgdk_pixbuf-2.0.so.0")
    glib = ctypes.CDLL("libglib-2.0.so.0")
    gobject = ctypes.CDLL("libgobject-2.0.so.0")
    pixbuf.gdk_pixbuf_new_from_file.argtypes = [
        ctypes.c_char_p,
        ctypes.POINTER(ctypes.POINTER(GError)),
    ]
    pixbuf.gdk_pixbuf_new_from_file.restype = ctypes.c_void_p
    glib.g_error_free.argtypes = [ctypes.POINTER(GError)]
    glib.g_error_free.restype = None
    gobject.g_object_unref.argtypes = [ctypes.c_void_p]
    gobject.g_object_unref.restype = None

    error = ctypes.POINTER(GError)()
    image = pixbuf.gdk_pixbuf_new_from_file(bytes(icon), ctypes.byref(error))
    if not image:
        message = error.contents.message.decode("utf-8", errors="replace") if error else "no error details"
        print(f"Native SVG loader failed: {message}", file=sys.stderr)
        if error:
            glib.g_error_free(error)
        return 1
    gobject.g_object_unref(image)
    print("Native SVG loader: OK", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
