"""A fake game for the positional audio E2E tests (AC-1.31, AC-1.55): writes a Mumble Link
`LinkedMem` (version 2, Linux layout with 4-byte wchar_t) into the shared memory segment
`/MumbleLink.<uid>` and keeps bumping `uiTick` like a running game, until it is killed.

The avatar and camera stand at the origin, look along +Z and have +Y up, so a person at
negative X is on the left. Usage: fake-link.py <game name> <context>
"""

import mmap
import os
import signal
import struct
import sys
import time

SIZE = 10580  # sizeof(LinkedMem) with 4-byte wchar_t
NAME_AT, CAMERA_AT, IDENTITY_AT, CONTEXT_LEN_AT, CONTEXT_AT = 44, 1068, 1104, 2128, 2132


def wide(text: str, chars: int) -> bytes:
    return text.encode("utf-32-le")[: (chars - 1) * 4].ljust(chars * 4, b"\0")


def main() -> None:
    name, context = sys.argv[1], sys.argv[2].encode()
    path = f"/dev/shm/MumbleLink.{os.getuid()}"
    fd = os.open(path, os.O_RDWR | os.O_CREAT, 0o600)
    if os.fstat(fd).st_size < SIZE:
        os.ftruncate(fd, SIZE)
    mem = mmap.mmap(fd, SIZE)
    os.close(fd)

    pose = struct.pack("<9f", 0, 0, 0, 0, 0, 1, 0, 1, 0)
    mem[8:44] = pose
    mem[NAME_AT : NAME_AT + 1024] = wide(name, 256)
    mem[CAMERA_AT : CAMERA_AT + 36] = pose
    mem[IDENTITY_AT : IDENTITY_AT + 1024] = wide("e2e", 256)
    mem[CONTEXT_LEN_AT : CONTEXT_LEN_AT + 4] = struct.pack("<I", len(context))
    mem[CONTEXT_AT : CONTEXT_AT + 256] = context.ljust(256, b"\0")
    mem[0:4] = struct.pack("<I", 2)

    # Like a closing game: stop ticking, so the client drops the game after its timeout.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
    print("ready", flush=True)
    tick = 0
    while True:
        tick += 1
        mem[4:8] = struct.pack("<I", tick)
        time.sleep(0.05)


if __name__ == "__main__":
    main()
