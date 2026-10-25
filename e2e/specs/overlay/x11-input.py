#!/usr/bin/env python3
"""Reports how much pointer input an X11 window takes, for the overlay's click-through check
(AC-1.33, QA.61): the number of rectangles in its input shape (0 = clicks always pass through)
and whether some client holds a pointer grab (which would keep clicks from other windows).
Uses ctypes so the E2E image needs no extra Python packages.
Usage: x11-input.py <title>   prints e.g. {"windows": 1, "inputRects": 0, "pointerGrabbed": false}"""
import ctypes
import ctypes.util
import json
import subprocess
import sys

SHAPE_INPUT = 2
GRAB_SUCCESS = 0
ALREADY_GRABBED = 1

title = sys.argv[1]
ids = subprocess.run(["xdotool", "search", "--onlyvisible", "--name", f"^{title}$"],
                     capture_output=True, text=True).stdout.split()

x = ctypes.CDLL(ctypes.util.find_library("X11"))
xext = ctypes.CDLL(ctypes.util.find_library("Xext"))
x.XOpenDisplay.restype = ctypes.c_void_p
x.XOpenDisplay.argtypes = [ctypes.c_char_p]
x.XDefaultRootWindow.restype = ctypes.c_ulong
x.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
x.XGrabPointer.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_uint,
                           ctypes.c_int, ctypes.c_int, ctypes.c_ulong, ctypes.c_ulong,
                           ctypes.c_ulong]
x.XUngrabPointer.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
x.XFree.argtypes = [ctypes.c_void_p]
x.XFlush.argtypes = [ctypes.c_void_p]
xext.XShapeGetRectangles.restype = ctypes.c_void_p
xext.XShapeGetRectangles.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int,
                                     ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_int)]

dpy = x.XOpenDisplay(None)
rects = 0
for wid in ids:
    count = ctypes.c_int(0)
    ordering = ctypes.c_int(0)
    ptr = xext.XShapeGetRectangles(dpy, int(wid), SHAPE_INPUT, ctypes.byref(count),
                                   ctypes.byref(ordering))
    if ptr:
        x.XFree(ptr)
    rects += count.value

# A pointer grab by any other client makes our own grab fail with AlreadyGrabbed.
status = x.XGrabPointer(dpy, x.XDefaultRootWindow(dpy), 0, 0, 1, 1, 0, 0, 0)
if status == GRAB_SUCCESS:
    x.XUngrabPointer(dpy, 0)
x.XFlush(dpy)
print(json.dumps({"windows": len(ids), "inputRects": rects,
                  "pointerGrabbed": status == ALREADY_GRABBED}))
