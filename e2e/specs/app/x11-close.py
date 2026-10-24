#!/usr/bin/env python3
"""Sends WM_DELETE_WINDOW to every X11 window with the given title, exactly like a window
manager does when the user clicks the close button. Xvfb has no WM, so xdotool cannot do this.
Usage: x11-close.py <title>"""
import ctypes
import ctypes.util
import subprocess
import sys

title = sys.argv[1]
ids = subprocess.run(["xdotool", "search", "--name", f"^{title}$"], capture_output=True,
                     text=True).stdout.split()
x = ctypes.CDLL(ctypes.util.find_library("X11"))
x.XOpenDisplay.restype = ctypes.c_void_p
x.XOpenDisplay.argtypes = [ctypes.c_char_p]
x.XInternAtom.restype = ctypes.c_ulong
x.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
x.XSendEvent.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_long,
                         ctypes.c_void_p]
x.XFlush.argtypes = [ctypes.c_void_p]


class ClientMessage(ctypes.Structure):
    _fields_ = [("type", ctypes.c_int), ("serial", ctypes.c_ulong), ("send_event", ctypes.c_int),
                ("display", ctypes.c_void_p), ("window", ctypes.c_ulong),
                ("message_type", ctypes.c_ulong), ("format", ctypes.c_int),
                ("l", ctypes.c_long * 5)]


class XEvent(ctypes.Union):
    _fields_ = [("xclient", ClientMessage), ("pad", ctypes.c_long * 24)]


dpy = x.XOpenDisplay(None)
protocols = x.XInternAtom(dpy, b"WM_PROTOCOLS", 0)
delete = x.XInternAtom(dpy, b"WM_DELETE_WINDOW", 0)
sent = 0
for wid in ids:
    ev = XEvent()
    ev.xclient.type = 33  # ClientMessage
    ev.xclient.window = int(wid)
    ev.xclient.message_type = protocols
    ev.xclient.format = 32
    ev.xclient.l[0] = delete
    ev.xclient.l[1] = 0  # CurrentTime
    x.XSendEvent(dpy, int(wid), 0, 0, ctypes.byref(ev))
    sent += 1
x.XFlush(dpy)
print(sent)
