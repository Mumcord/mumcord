#!/usr/bin/env python3
"""Checks that a window stays above a program filling the whole screen (AC-1.33): opens a window
covering the screen, activates and raises it like a game that just got focus, then compares
the stacking order of the top-level windows (their window manager frames). Needs a running
EWMH window manager. Uses ctypes so the E2E image needs no extra Python packages.

`borderless` asks for no decorations (_MOTIF_WM_HINTS), like a borderless-fullscreen game. Window
managers may treat such a window as fullscreen and stack it above keep-above windows (openbox
does), so that case depends on the window manager.
Usage: x11-on-top.py <title> decorated|borderless   prints e.g. {"managed": true, "above": true, ...}"""
import ctypes
import ctypes.util
import json
import subprocess
import sys
import time

title, mode = sys.argv[1], sys.argv[2]
x = ctypes.CDLL(ctypes.util.find_library("X11"))
c_ulong_p = ctypes.POINTER(ctypes.c_ulong)
x.XOpenDisplay.restype = ctypes.c_void_p
x.XOpenDisplay.argtypes = [ctypes.c_char_p]
x.XDefaultRootWindow.restype = ctypes.c_ulong
x.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
x.XDisplayWidth.argtypes = [ctypes.c_void_p, ctypes.c_int]
x.XDisplayHeight.argtypes = [ctypes.c_void_p, ctypes.c_int]
x.XInternAtom.restype = ctypes.c_ulong
x.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
x.XCreateSimpleWindow.restype = ctypes.c_ulong
x.XCreateSimpleWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_int,
                                  ctypes.c_uint, ctypes.c_uint, ctypes.c_uint, ctypes.c_ulong,
                                  ctypes.c_ulong]
x.XStoreName.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_char_p]
x.XChangeProperty.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_ulong,
                              ctypes.c_int, ctypes.c_int, ctypes.c_void_p, ctypes.c_int]
x.XMapRaised.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
x.XRaiseWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
x.XFlush.argtypes = [ctypes.c_void_p]
x.XFree.argtypes = [ctypes.c_void_p]
x.XGetAtomName.restype = ctypes.c_void_p
x.XGetAtomName.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
x.XQueryTree.argtypes = [ctypes.c_void_p, ctypes.c_ulong, c_ulong_p, c_ulong_p,
                         ctypes.POINTER(c_ulong_p), ctypes.POINTER(ctypes.c_uint)]
x.XGetWindowProperty.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_long,
                                 ctypes.c_long, ctypes.c_int, ctypes.c_ulong, c_ulong_p,
                                 ctypes.POINTER(ctypes.c_int), c_ulong_p, c_ulong_p,
                                 ctypes.POINTER(ctypes.c_void_p)]


class ClientMessage(ctypes.Structure):
    _fields_ = [("type", ctypes.c_int), ("serial", ctypes.c_ulong), ("send_event", ctypes.c_int),
                ("display", ctypes.c_void_p), ("window", ctypes.c_ulong),
                ("message_type", ctypes.c_ulong), ("format", ctypes.c_int),
                ("data", ctypes.c_long * 5), ("pad", ctypes.c_long * 12)]


x.XSendEvent.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_long,
                         ctypes.POINTER(ClientMessage)]
CLIENT_MESSAGE = 33
SUBSTRUCTURE_REDIRECT_AND_NOTIFY = (1 << 20) | (1 << 19)
XA_ATOM, XA_WINDOW = 4, 33
PROP_MODE_REPLACE = 0

dpy = x.XOpenDisplay(None)
root = x.XDefaultRootWindow(dpy)
atom = lambda name: x.XInternAtom(dpy, name.encode(), 0)


def ids_property(window: int, name: str, kind: int) -> list[int]:
    actual_type, actual_format = ctypes.c_ulong(), ctypes.c_int()
    count, after, data = ctypes.c_ulong(), ctypes.c_ulong(), ctypes.c_void_p()
    x.XGetWindowProperty(dpy, window, atom(name), 0, 4096, 0, kind,
                         ctypes.byref(actual_type), ctypes.byref(actual_format),
                         ctypes.byref(count), ctypes.byref(after), ctypes.byref(data))
    if not data.value:
        return []
    ids = list(ctypes.cast(data, c_ulong_p)[: count.value])
    x.XFree(data)
    return ids


def stacking() -> list[int]:
    return ids_property(root, "_NET_CLIENT_LIST_STACKING", XA_WINDOW)


def wm_state(window: int) -> list[str]:
    """_NET_WM_STATE without the prefix, e.g. ["ABOVE"], for the failure report."""
    names = []
    for a in ids_property(window, "_NET_WM_STATE", XA_ATOM):
        p = x.XGetAtomName(dpy, a)
        names.append(ctypes.string_at(p).decode().removeprefix("_NET_WM_STATE_"))
        x.XFree(p)
    return names


def tree(window: int) -> tuple[int, list[int]]:
    """(parent, children bottom to top)."""
    root_ret, parent, children, count = ctypes.c_ulong(), ctypes.c_ulong(), c_ulong_p(), ctypes.c_uint()
    x.XQueryTree(dpy, window, ctypes.byref(root_ret), ctypes.byref(parent), ctypes.byref(children),
                 ctypes.byref(count))
    kids = list(children[: count.value]) if count.value else []
    if count.value:
        x.XFree(children)
    return parent.value, kids


def top_level(window: int) -> int:
    """The direct child of the root that holds `window` (its frame, if managed)."""
    while True:
        parent, _ = tree(window)
        if parent in (root, 0):
            return window
        window = parent


game = x.XCreateSimpleWindow(dpy, root, 0, 0, x.XDisplayWidth(dpy, 0), x.XDisplayHeight(dpy, 0),
                             0, 0, 0x2040c0)
x.XStoreName(dpy, game, b"Fullscreen game")
if mode == "borderless":
    # _MOTIF_WM_HINTS: flags = decorations, decorations = none (how borderless games ask for it).
    hints = (ctypes.c_long * 5)(2, 0, 0, 0, 0)
    motif = atom("_MOTIF_WM_HINTS")
    x.XChangeProperty(dpy, game, motif, motif, 32, PROP_MODE_REPLACE, hints, 5)
x.XMapRaised(dpy, game)
x.XFlush(dpy)

managed = False
for _ in range(50):
    if game in stacking():
        managed = True
        break
    time.sleep(0.1)

# Focus and raise it, like a game the user just clicked into.
event = ClientMessage(type=CLIENT_MESSAGE, send_event=1, display=dpy, window=game,
                      message_type=atom("_NET_ACTIVE_WINDOW"), format=32)
event.data[0] = 2  # source: pager / direct user action
x.XSendEvent(dpy, root, 0, SUBSTRUCTURE_REDIRECT_AND_NOTIFY, ctypes.byref(event))
x.XRaiseWindow(dpy, game)
x.XFlush(dpy)
time.sleep(1)

order = tree(root)[1]
overlays = [int(w) for w in subprocess.run(
    ["xdotool", "search", "--onlyvisible", "--name", f"^{title}$"],
    capture_output=True, text=True).stdout.split()]
overlay_at = [order.index(top_level(w)) for w in overlays]
above = managed and bool(overlay_at) and min(overlay_at) > order.index(top_level(game))
print(json.dumps({"managed": managed, "overlays": len(overlay_at), "above": above,
                  "overlayState": [wm_state(w) for w in overlays], "gameState": wm_state(game)}))
