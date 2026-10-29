#!/usr/bin/env python3
"""Reads the X11 CLIPBOARD the way a pasting application does, so a test can check what the app
copied without going through the webview's own paste. Prints the text for `text`; for `png` it
prints `png <bytes>` when the clipboard offers a PNG image. Exits 2 when there is nothing.
Usage: x11-clipboard.py text|png"""
import ctypes
import ctypes.util
import sys
import time

mode = sys.argv[1]
x = ctypes.CDLL(ctypes.util.find_library("X11"))
x.XOpenDisplay.restype = ctypes.c_void_p
x.XOpenDisplay.argtypes = [ctypes.c_char_p]
x.XDefaultRootWindow.restype = ctypes.c_ulong
x.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
x.XCreateSimpleWindow.restype = ctypes.c_ulong
x.XCreateSimpleWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong] + [ctypes.c_int] * 2 + \
    [ctypes.c_uint] * 3 + [ctypes.c_ulong] * 2
x.XInternAtom.restype = ctypes.c_ulong
x.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
x.XConvertSelection.argtypes = [ctypes.c_void_p] + [ctypes.c_ulong] * 5
x.XPending.argtypes = [ctypes.c_void_p]
x.XNextEvent.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
x.XGetWindowProperty.argtypes = [
    ctypes.c_void_p, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_long, ctypes.c_long, ctypes.c_int,
    ctypes.c_ulong, ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_int),
    ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_ulong),
    ctypes.POINTER(ctypes.POINTER(ctypes.c_ubyte))]


class SelectionNotify(ctypes.Structure):
    _fields_ = [("type", ctypes.c_int), ("serial", ctypes.c_ulong), ("send_event", ctypes.c_int),
                ("display", ctypes.c_void_p), ("requestor", ctypes.c_ulong),
                ("selection", ctypes.c_ulong), ("target", ctypes.c_ulong),
                ("property", ctypes.c_ulong), ("time", ctypes.c_ulong)]


class XEvent(ctypes.Union):
    _fields_ = [("type", ctypes.c_int), ("selection", SelectionNotify), ("pad", ctypes.c_long * 24)]


SELECTION_NOTIFY = 31
display = x.XOpenDisplay(None)
if not display:
    sys.exit("cannot open the X display")
window = x.XCreateSimpleWindow(display, x.XDefaultRootWindow(display), 0, 0, 1, 1, 0, 0, 0)
atom = lambda name: x.XInternAtom(display, name.encode(), 0)
target = atom("UTF8_STRING" if mode == "text" else "image/png")
prop = atom("E2E_CLIPBOARD")
x.XConvertSelection(display, atom("CLIPBOARD"), target, prop, window, 0)

event = XEvent()
deadline = time.monotonic() + 5
while True:
    if x.XPending(display):
        x.XNextEvent(display, ctypes.byref(event))
        if event.type == SELECTION_NOTIFY:
            break
    elif time.monotonic() > deadline:
        sys.exit("the clipboard owner did not answer")
    else:
        time.sleep(0.05)
if event.selection.property == 0:
    sys.exit(2)

kind, fmt, items, after = ctypes.c_ulong(), ctypes.c_int(), ctypes.c_ulong(), ctypes.c_ulong()
data = ctypes.POINTER(ctypes.c_ubyte)()
x.XGetWindowProperty(display, window, prop, 0, 1 << 24, 0, 0, ctypes.byref(kind),
                     ctypes.byref(fmt), ctypes.byref(items), ctypes.byref(after),
                     ctypes.byref(data))
raw = ctypes.string_at(data, items.value) if items.value else b""
if kind.value == atom("INCR"):
    sys.exit("incremental transfers are not supported")
if mode == "text":
    sys.stdout.write(raw.decode("utf-8"))
elif raw.startswith(b"\x89PNG\r\n\x1a\n"):
    print(f"png {len(raw)}")
else:
    sys.exit(2)
