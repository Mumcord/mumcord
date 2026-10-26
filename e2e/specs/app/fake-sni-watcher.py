#!/usr/bin/env python3
"""Owns org.kde.StatusNotifierWatcher on the session bus so the app believes a system tray host
is present (Xvfb has none). Pure-python D-Bus (no bindings in the image). It holds the name until
killed and answers every method call with an empty reply, except IsStatusNotifierHostRegistered
(true): a tray started while the name is owned registers with it and waits for that reply, so a
silent stand-in would leave the app without a tray. Prints "ready" once the name is owned."""
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


def reply(serial, reply_to, dest, sig="", body=b""):
    fields = pad(struct.pack("<B", 5) + string("u", True), 4) + struct.pack("<I", reply_to)
    fields = pad(fields, 8) + struct.pack("<B", 6) + string("s", True) + string(dest)
    if sig:
        fields = pad(fields, 8) + struct.pack("<B", 8) + string("g", True) + string(sig, True)
    head = struct.pack("<cBBBII", b"l", 2, 0, 1, len(body), serial)
    head += struct.pack("<I", len(fields)) + fields
    return pad(head, 8) + body


def header_fields(raw):
    """Header field code -> string value (only the string-typed fields are needed here)."""
    out, i = {}, 0
    while i < len(raw):
        i += (-i) % 8
        code, siglen = raw[i], raw[i + 1]
        typ = raw[i + 2:i + 2 + siglen].decode()
        i += 3 + siglen
        if typ in ("s", "o"):
            i += (-i) % 4
            (n,) = struct.unpack_from("<I", raw, i)
            out[code] = raw[i + 4:i + 4 + n].decode()
            i += 5 + n
        elif typ == "g":
            n = raw[i]
            out[code] = raw[i + 1:i + 1 + n].decode()
            i += 2 + n
        else:  # "u"
            i += (-i) % 4 + 4
    return out


print("ready", flush=True)
buf, serial = b"", 10
while True:
    chunk = sock.recv(65536)
    if not chunk:
        break
    buf += chunk
    while len(buf) >= 16:
        _, kind, flags, _, body_len, msg_serial, fields_len = struct.unpack_from("<cBBBIII", buf)
        start = 16 + fields_len + (-(16 + fields_len)) % 8
        if len(buf) < start + body_len:
            break
        fields, body = header_fields(buf[16:16 + fields_len]), buf[start:start + body_len]
        buf = buf[start + body_len:]
        if kind != 1 or flags & 1 or 7 not in fields:  # only method calls that want a reply
            continue
        serial += 1
        if fields.get(3) == "Get" and b"IsStatusNotifierHostRegistered" in body:
            sock.sendall(reply(serial, msg_serial, fields[7], "v", string("b", True) + b"\0" + struct.pack("<I", 1)))
        else:
            sock.sendall(reply(serial, msg_serial, fields[7]))
