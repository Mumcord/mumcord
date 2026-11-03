#!/usr/bin/env python3
"""Drags files onto an application window the way a file manager does: a real XDND drag source
(XdndEnter, XdndPosition, XdndDrop, then the text/uri-list on the XdndSelection) aimed at the
top-level window of process PID, at window-relative point X,Y. WebDriver cannot drag files from
outside the page, and this goes through the same GTK drop path as a real drag.
With --leave it moves over the point and leaves again instead of dropping (the drop overlay).
Usage: x11-drop.py PID X Y PATH... [--leave]
Exits 0 when the target accepted and finished the drop."""
import ctypes
import ctypes.util
import sys
import time
import urllib.parse

args = [a for a in sys.argv[1:] if a != "--leave"]
leave_only = "--leave" in sys.argv[1:]
pid, wx, wy, paths = int(args[0]), int(args[1]), int(args[2]), args[3:]

x = ctypes.CDLL(ctypes.util.find_library("X11"))
Window = ctypes.c_ulong
Atom = ctypes.c_ulong
x.XOpenDisplay.restype = ctypes.c_void_p
x.XOpenDisplay.argtypes = [ctypes.c_char_p]
x.XDefaultRootWindow.restype = Window
x.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
x.XCreateSimpleWindow.restype = Window
x.XCreateSimpleWindow.argtypes = [ctypes.c_void_p, Window] + [ctypes.c_int] * 2 + \
    [ctypes.c_uint] * 3 + [ctypes.c_ulong] * 2
x.XInternAtom.restype = Atom
x.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
x.XSetSelectionOwner.argtypes = [ctypes.c_void_p, Atom, Window, ctypes.c_ulong]
x.XPending.argtypes = [ctypes.c_void_p]
x.XNextEvent.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
x.XSendEvent.argtypes = [ctypes.c_void_p, Window, ctypes.c_int, ctypes.c_long, ctypes.c_void_p]
x.XFlush.argtypes = [ctypes.c_void_p]
x.XQueryTree.argtypes = [ctypes.c_void_p, Window, ctypes.POINTER(Window), ctypes.POINTER(Window),
                         ctypes.POINTER(ctypes.POINTER(Window)), ctypes.POINTER(ctypes.c_uint)]
x.XFree.argtypes = [ctypes.c_void_p]
x.XGetWindowProperty.argtypes = [
    ctypes.c_void_p, Window, Atom, ctypes.c_long, ctypes.c_long, ctypes.c_int, Atom,
    ctypes.POINTER(Atom), ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_ulong),
    ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.POINTER(ctypes.c_ubyte))]
x.XChangeProperty.argtypes = [ctypes.c_void_p, Window, Atom, Atom, ctypes.c_int, ctypes.c_int,
                              ctypes.c_char_p, ctypes.c_int]
x.XTranslateCoordinates.argtypes = [ctypes.c_void_p, Window, Window, ctypes.c_int, ctypes.c_int,
                                    ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_int),
                                    ctypes.POINTER(Window)]


class ClientMessage(ctypes.Structure):
    _fields_ = [("type", ctypes.c_int), ("serial", ctypes.c_ulong), ("send_event", ctypes.c_int),
                ("display", ctypes.c_void_p), ("window", Window), ("message_type", Atom),
                ("format", ctypes.c_int), ("l", ctypes.c_long * 5)]


class SelectionRequest(ctypes.Structure):
    _fields_ = [("type", ctypes.c_int), ("serial", ctypes.c_ulong), ("send_event", ctypes.c_int),
                ("display", ctypes.c_void_p), ("owner", Window), ("requestor", Window),
                ("selection", Atom), ("target", Atom), ("property", Atom),
                ("time", ctypes.c_ulong)]


class SelectionNotify(ctypes.Structure):
    _fields_ = [("type", ctypes.c_int), ("serial", ctypes.c_ulong), ("send_event", ctypes.c_int),
                ("display", ctypes.c_void_p), ("requestor", Window), ("selection", Atom),
                ("target", Atom), ("property", Atom), ("time", ctypes.c_ulong)]


class XEvent(ctypes.Union):
    _fields_ = [("type", ctypes.c_int), ("client", ClientMessage),
                ("request", SelectionRequest), ("notify", SelectionNotify),
                ("pad", ctypes.c_long * 24)]


CLIENT_MESSAGE, SELECTION_REQUEST, SELECTION_NOTIFY = 33, 30, 31
display = x.XOpenDisplay(None)
if not display:
    sys.exit("cannot open the X display")
root = x.XDefaultRootWindow(display)
atom = lambda name: x.XInternAtom(display, name.encode(), 0)
source = x.XCreateSimpleWindow(display, root, 0, 0, 1, 1, 0, 0, 0)


def prop_values(window, name):
    kind, fmt, items, after = Atom(), ctypes.c_int(), ctypes.c_ulong(), ctypes.c_ulong()
    data = ctypes.POINTER(ctypes.c_ubyte)()
    if x.XGetWindowProperty(display, window, atom(name), 0, 1024, 0, 0, ctypes.byref(kind),
                            ctypes.byref(fmt), ctypes.byref(items), ctypes.byref(after),
                            ctypes.byref(data)) != 0 or not data:
        return []
    values = [ctypes.cast(data, ctypes.POINTER(ctypes.c_ulong))[i] for i in range(items.value)] \
        if fmt.value == 32 else []
    x.XFree(data)
    return values


def children(window):
    root_ret, parent = Window(), Window()
    kids, count = ctypes.POINTER(Window)(), ctypes.c_uint()
    if not x.XQueryTree(display, window, ctypes.byref(root_ret), ctypes.byref(parent),
                        ctypes.byref(kids), ctypes.byref(count)):
        return []
    out = [kids[i] for i in range(count.value)]
    if kids:
        x.XFree(kids)
    return out


class WindowAttributes(ctypes.Structure):
    _fields_ = [("x", ctypes.c_int), ("y", ctypes.c_int), ("width", ctypes.c_int),
                ("height", ctypes.c_int), ("border_width", ctypes.c_int), ("depth", ctypes.c_int),
                ("visual", ctypes.c_void_p), ("root", Window), ("class", ctypes.c_int),
                ("bit_gravity", ctypes.c_int), ("win_gravity", ctypes.c_int),
                ("backing_store", ctypes.c_int), ("backing_planes", ctypes.c_ulong),
                ("backing_pixel", ctypes.c_ulong), ("save_under", ctypes.c_int),
                ("colormap", ctypes.c_ulong), ("map_installed", ctypes.c_int),
                ("map_state", ctypes.c_int), ("all_event_masks", ctypes.c_long),
                ("your_event_mask", ctypes.c_long), ("do_not_propagate_mask", ctypes.c_long),
                ("override_redirect", ctypes.c_int), ("screen", ctypes.c_void_p)]


x.XGetWindowAttributes.argtypes = [ctypes.c_void_p, Window, ctypes.POINTER(WindowAttributes)]
IS_VIEWABLE = 2


def candidates(window, found):
    """Drop-aware windows of the process; GTK keeps the top-level below the WM frame."""
    if pid in prop_values(window, "_NET_WM_PID") and prop_values(window, "XdndAware"):
        attrs = WindowAttributes()
        if x.XGetWindowAttributes(display, window, ctypes.byref(attrs)) and \
                attrs.map_state == IS_VIEWABLE:
            found.append((attrs.width * attrs.height, window))
    for child in children(window):
        candidates(child, found)
    return found


def find_target(window):
    """The main window: the largest shown one (the app also has hidden helper windows)."""
    found = candidates(window, [])
    return max(found)[1] if found else None


target = find_target(root)
if not target:
    sys.exit(f"no drop-aware window of process {pid}")
rx, ry, _child = ctypes.c_int(), ctypes.c_int(), Window()
x.XTranslateCoordinates(display, target, root, wx, wy, ctypes.byref(rx), ctypes.byref(ry),
                        ctypes.byref(_child))

# The pointer goes where the drag is, as with a real mouse.
x.XWarpPointer.argtypes = [ctypes.c_void_p, Window, Window] + [ctypes.c_int] * 2 + \
    [ctypes.c_uint] * 2 + [ctypes.c_int] * 2
x.XWarpPointer(display, 0, root, 0, 0, 0, 0, rx.value, ry.value)
x.XFlush(display)

uri_list = atom("text/uri-list")
selection = atom("XdndSelection")
x.XSetSelectionOwner(display, selection, source, 0)
payload = "".join("file://" + urllib.parse.quote(p) + "\r\n" for p in paths).encode()


def send(kind, l1=0, l2=0, l3=0, l4=0):
    event = XEvent()
    event.client.type = CLIENT_MESSAGE
    event.client.window = target
    event.client.message_type = atom(kind)
    event.client.format = 32
    event.client.l[0], event.client.l[1], event.client.l[2] = source, l1, l2
    event.client.l[3], event.client.l[4] = l3, l4
    x.XSendEvent(display, target, 0, 0, ctypes.byref(event))
    x.XFlush(display)


def wait_for(kind, timeout=5.0):
    """Serves selection requests while waiting for the client message `kind` from the target."""
    event = XEvent()
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if not x.XPending(display):
            time.sleep(0.02)
            continue
        x.XNextEvent(display, ctypes.byref(event))
        if event.type == SELECTION_REQUEST:
            req = event.request
            reply = XEvent()
            reply.notify.type = SELECTION_NOTIFY
            reply.notify.requestor, reply.notify.selection = req.requestor, req.selection
            reply.notify.target, reply.notify.time = req.target, req.time
            reply.notify.property = 0
            if req.target == uri_list:
                x.XChangeProperty(display, req.requestor, req.property, uri_list, 8, 0, payload,
                                  len(payload))
                reply.notify.property = req.property
            x.XSendEvent(display, req.requestor, 0, 0, ctypes.byref(reply))
            x.XFlush(display)
        elif event.type == CLIENT_MESSAGE and event.client.message_type == atom(kind):
            return event.client
    return None


# XDND version 5, one offered type (text/uri-list) in the message itself.
send("XdndEnter", 5 << 24, uri_list)
# WebKit answers a position at once with "no" and decides only after the page saw the drag, so
# keep moving like a real pointer until a status accepts the files.
accepted = False
seen = []
deadline = time.monotonic() + 15
while not accepted and time.monotonic() < deadline:
    send("XdndPosition", 0, (rx.value << 16) | (ry.value & 0xFFFF), 0, atom("XdndActionCopy"))
    status = wait_for("XdndStatus", 2.0)
    accepted = status is not None and bool(status.l[1] & 1)
    seen.append("none" if status is None else str(status.l[1]))
    time.sleep(0.3)
if not accepted:
    send("XdndLeave")
    sys.exit(f"the window did not accept the files (status flags {seen})")
# The page shows its drop cover on the following moves, as during a real drag.
for _ in range(10 if leave_only else 3):
    send("XdndPosition", 0, (rx.value << 16) | (ry.value & 0xFFFF), 0, atom("XdndActionCopy"))
    wait_for("XdndStatus", 1.0)
    time.sleep(0.3)
if leave_only:
    send("XdndLeave")
    sys.exit(0)
send("XdndDrop", 0, 0)
if wait_for("XdndFinished", 10.0) is None:
    sys.exit("the window did not finish the drop")
