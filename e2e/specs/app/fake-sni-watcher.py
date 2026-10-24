#!/usr/bin/env python3
"""Owns org.kde.StatusNotifierWatcher on the session bus so the app believes a system tray host
is present (Xvfb has none). Pure-python D-Bus (no bindings in the image). Answers nothing; it
only holds the name until killed. Prints "ready" once the name is owned."""
import os
import socket
import struct
import sys


def pad(buf, n):
    return buf + b"\0" * ((-len(buf)) % n)


def string(s, sig=False):
    b = s.encode()
    return (struct.pack("<B", len(b)) if sig else struct.pack("<I", len(b))) + b + b"\0"


def message(serial, path, iface, member, dest, sig="", body=b""):
    fields = b""
    for code, typ, val in [(1, "o", path), (2, "s", iface), (3, "s", member), (6, "s", dest)] + (
            [(8, "g", sig)] if sig else []):
        fields = pad(fields, 8)
        fields += struct.pack("<B", code) + string(typ, True)
        fields += string(val, True) if typ == "g" else string(val)
    head = struct.pack("<cBBBII", b"l", 1, 0, 1, len(body), serial)
    head += struct.pack("<I", len(fields)) + fields
    return pad(head, 8) + body


addr = os.environ["DBUS_SESSION_BUS_ADDRESS"]
path = [kv.split("=", 1)[1] for kv in addr.split(",") if kv.startswith(("unix:path=", "path="))]
sock = socket.socket(socket.AF_UNIX)
if path:
    sock.connect(path[0])
else:  # unix:abstract=
    abstract = [kv.split("=", 1)[1] for kv in addr.split(",") if "abstract=" in kv][0]
    sock.connect("\0" + abstract)
sock.sendall(b"\0AUTH EXTERNAL " + str(os.getuid()).encode().hex().encode() + b"\r\n")
if not sock.recv(4096).startswith(b"OK"):
    sys.exit("dbus auth failed")
sock.sendall(b"BEGIN\r\n")
bus = ("/org/freedesktop/DBus", "org.freedesktop.DBus")
sock.sendall(message(1, bus[0], bus[1], "Hello", bus[1]))
body = pad(string("org.kde.StatusNotifierWatcher"), 4) + struct.pack("<I", 4)  # DO_NOT_QUEUE
sock.sendall(message(2, bus[0], bus[1], "RequestName", bus[1], "su", body))
print("ready", flush=True)
while sock.recv(65536):
    pass
