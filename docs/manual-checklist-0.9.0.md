# Release 0.9.0: manual test checklist

Everything the 0.9.0 acceptance run (task E4) needs a human, an 0.8.0 build, `sudo`, a capture on
the relay host or several machines for. The automated criteria are covered by the E2E suite
(`e2e/README.md`), the engine live tests and the vitest suites; the per-AC evidence and the exact
commands are in
[`specs/07-08-bovnar-media-streams-0.9.0-acceptance-inventory.md`](../specs/07-08-bovnar-media-streams-0.9.0-acceptance-inventory.md).
Do not repeat an automated check here, and do not leave a check of this list undone because the
suite is green.

Rules for every check:

- Use a local test server (`testenv/README.md`), a local mumpeer from `../mumpeer` and its
  announcing bot. Never a public or third-party Mumble server.
- Use the build you want to release: the CI artefacts of the E4 commit, or a local
  `npm run tauri build`. Checks that name a test hook need a **debug** build (`tauri build
--debug --no-bundle`, or `npm run tauri dev`), because the hooks are inert in release builds.
- For the 0.8.x side use a real 0.8.0 build (tag `v0.8.0`, which contains P0.5). Run it next to
  0.9.0 with its own `XDG_RUNTIME_DIR`, `XDG_DATA_HOME` and `XDG_CONFIG_HOME` — the single-instance
  lock is a socket in `XDG_RUNTIME_DIR` — or on a second machine if that is simpler.
- Write down per check: OS and version, the build (0.9.0 or 0.8.0, release or debug), the relay
  profile from `/v1/status`, pass or fail, and on a failure what you saw. Attach the Mumcord log
  (_Settings → Diagnostics_); it contains no chat, names, addresses or audio.
- A failure becomes its own fix task and a new run; never fix and verify in the same session.

## 1. Setup and preconditions (about 30 min)

- [ ] **Relay media profile (AC-7.72, AC-8.30):** start the mumpeer testenv with `MUMPEER_PROFILE=all` and read `/v1/status` with the API secret: `.media` is `2` and `.broadcast.max_viewers` is `25`. Write the value down; every new-format check below depends on it.
- [ ] **Old relay (AC-7.78, AC-8.40):** start the previous mumpeer release (`MUMPEER_IMAGE` and `MUMBOT_IMAGE` of the last release, `MUMPEER_PROFILE=old`): `/v1/status` has **no** `media` field.
- [ ] **Two builds side by side (all 0.8.x checks):** the 0.8.0 build and the 0.9.0 build run at the same time in the same channel of the local server, each with its own identity and data folder.
- [ ] **Test camera (camera checks):** with `MUMCORD_TEST_CAMERA=1` the preview shows the fixture person; without it the real webcam is used. Note in the report which one each camera check used.

## 2. Screen share for a 0.8.x viewer (0.8.0 + 0.9.0 client; about 45 min)

Relay profile `old` first, then `all`.

- [ ] **AC-7.78 old relay, old format:** A (0.9.0) shares; D (0.8.0) clicks Watch. D sees the share; A's packet inspector shows the old header row (`.media legacy v1` or `v2`, no channel), and no new-format row is sent to D.
- [ ] **AC-7.79 new relay, old-format direct copy:** profile `all`; A shares, D watches and sees the picture. A's inspector shows a direct copy for D, and `/v1/status` shows the one broadcast, not a second one for D.
- [ ] **AC-7.79 direct slots full:** with 8 old-format direct copies in use, a ninth 0.8.0 viewer gets "This share needs Mumcord 0.9.0" instead of a picture. With only two 0.8.0 viewers both get their own copy.
- [ ] **AC-7.88 0.9.0 viewer of a 0.8.x share:** D (0.8.0) shares, A (0.9.0) watches, once on profile `old` and once on `all`. A sees the picture, its inspector shows the old header, no error appears and the format never changes mid-share.
- [ ] **AC-7.81 old clients on both relays:** two 0.8.0 clients share and watch on profile `old` and on `all`. It behaves as before; the relay log names the old format in the bind, and no broadcast mixes the two formats (read the bind lines and the per-broadcast subscriber list in the relay log).

## 3. Camera for a 0.8.x viewer (0.8.0 + 0.9.0 client; about 40 min)

- [ ] **AC-8.31 new relay, 0.8.0 viewer (`all`):** A (0.9.0) turns the camera on. D's (0.8.0) tile of A shows "This camera needs Mumcord 0.9.0" and no video, and D sends no further `cam-join` until A turns the camera off and on. A's inspector shows the `mumcord.camera` `needs` message to D, **no** `STREAM_CAMERA` opened for D, and `/v1/status` shows one camera broadcast, not two.
- [ ] **AC-8.31 `direct_only`:** A in _Direct only_ with the camera on; D still gets `needs` and the tile text, and no stream.
- [ ] **AC-8.31 old relay, 0.8.0 viewer (`old`):** A's camera runs over the old-format broadcast; D sees A's tile as before 0.9.0 and receives **no** `needs` message.
- [ ] **AC-8.31 0.9.0 viewer of a 0.8.x camera:** D (0.8.0) turns its camera on; A (0.9.0) sees the tile over the old path, on profile `old` and on `all`. A's inspector shows the old format and no error.
- [ ] **AC-8.40 old relay, 0.8.x viewer (`old`):** A's camera runs over the old-format relay broadcast; D sees the tile; A's inspector shows the old header and opens no direct (new-format) stream to D.
- [ ] **AC-8.41 two 0.8.0 clients (`old` and `all`):** the camera works as today on both relays; the new relay's bind names the old format and no broadcast mixes formats; neither client asks for an update or shows a camera error.

## 4. Relay host: blind relay (about 30 min)

Run the 0.9.0 clients A, B and C against relay profile `all`; capture on the relay host or inside
its container (`docker exec -it mumpeer-testenv-mumpeer-1 tcpdump -i any -w /tmp/media.pcap port
44759`).

- [ ] **AC-7.72 screen share:** A shares, B watches. In the capture, every media message carries only the channel, the flags and the SFrame header — no timestamp, no resolution, no frame sequence, no readable VP9/AV1 bitstream.
- [ ] **AC-7.76 rekey after a viewer leaves:** B stops watching; A rekeys (A's inspector shows a new key id and the new description) and the capture still shows only channel, flags and the SFrame header. A client started with `MUMCORD_TEST_KEEP_OLD_KEY=1` that held B's key logs that it cannot decrypt the new frames; C keeps watching without a black frame.
- [ ] **AC-8.37 camera:** same with A's camera on and B hiding A's video: A rekeys, the old key decrypts nothing, C's tile never goes black, and the capture shows the same limited view.

## 5. Forced TCP (about 30 min)

Both clients inside the namespaces of the mumpeer testenv (`sudo ../mumpeer/testenv/netns.sh up
noudp`, then start each client with `sudo -E ../mumpeer/testenv/netns.sh run a …` / `run b …`).

- [ ] **AC-7.86 screen share over TCP:** A shares, B watches. The picture plays, the relay log names TCP and shows `0x01` mux chunks carrying the media messages, a video message above 64 KiB arrives whole, B's "up to 15" choice still halves the rate, and a late joiner C starts at a keyframe.
- [ ] **AC-8.38 camera over TCP:** the same with the camera: A's tile arrives, late join and the tile-height `want` work, the layer cap still applies, and the relay log names TCP.

## 6. Keyframe request through the relay (about 20 min)

Three 0.9.0 clients against profile `all`, relay log open. The viewer asks for a keyframe by
itself after a decode gap, so cause one instead of hunting for a button: start the viewer behind
`tools/netsim` (see its `README.md`) and run `netsim ctl loss both 20` for a few seconds, then
`netsim ctl reset`.

- [ ] **AC-7.87 screen share:** A shares, B and C watch. With B behind netsim, drop 20 % of the UDP long enough for B's picture to stall and recover. A's inspector shows the forwarded request and a new keyframe on channels 1–4 within 1 s. The relay log shows at most one `kf` per 2 s per viewer and one per 500 ms per broadcast; repeating the loss produces no storm, B's picture recovers and C is unaffected.
- [ ] **AC-8.36 camera:** the same with the camera on and B's tile behind netsim: one `kf` per 2 s per viewer and per 500 ms per broadcast, a keyframe on channels 1–4 within 1 s, B's tile continues, C is unaffected.

## 7. Scale (about 40 min)

- [ ] **AC-8.30 25 camera viewers:** one camera broadcast with 25 viewers (`bcast-sim` in mumpeer 0.3.0 if it has camera publishers — confirm before the run; otherwise report the check as "not verified" and say which tool was missing). `/v1/status` shows a single camera broadcast; a 26th viewer is turned away; each viewer's path reads Relay.
- [ ] **AC-7.79 all 8 direct slots:** eight 0.8.0 viewers of a 0.9.0 share on profile `all`. The first eight get the old-format direct copy, the ninth gets "This share needs Mumcord 0.9.0", and A's upload is one copy per viewer with no relay broadcast for them.

## 8. Benchmark figures (idle machine; about 20 min)

- [ ] **AC-7.82:** `taskset -c 2 cargo bench -p mumcord-p2p --bench media`, then `cargo bench -p mumcord-engine --bench media_crypto`, on an otherwise idle machine. Copy the median nanoseconds per operation and the allocations per packet into the acceptance report, together with the accepted 400 KB keyframe caveat and the machine (CPU, OS).

## 9. Screenshots and looks (about 40 min)

Dark and light, 1440×900, 8-bit palette PNG into `docs/images/screenshots/` with a caption row in
that folder's `README.md`, and embedded in the PR description.

- [ ] **AC-7.84 camera room path:** the camera room header chip ("Direct · end-to-end encrypted", "Relay · end-to-end encrypted") and the viewer list with both paths, with a direct viewer and a relay viewer in the same room.
- [ ] **AC-7.84 needs-0.9.0 tile:** a camera tile in the "This camera needs Mumcord 0.9.0" state and a screen-share viewer in the "This share needs Mumcord 0.9.0" state.
- [ ] **AC-7.84 relay notice and inspector:** the "This server's relay needs an update" notice under the sharer's bar, and the packet inspector with a media row set: description on channel 0, frames on the layer channels, a `keyreq` and a `key` control row, and one old-header row.
- [ ] **AC-7.84 reading and keyboard:** with one non-English locale the new texts are translated and not truncated; keyboard only reaches the camera room controls and the inspector rows; a screen reader (Orca, NVDA or VoiceOver) reads the tile state and the path chips.

## 10. Leftovers (about 15 min)

- [ ] **Regression of the still-valid 0.5.0 checks:** sections 6 (cinema view and screen sharing) and 9 (packet inspector) of [docs/manual-checklist-0.5.0.md](manual-checklist-0.5.0.md), which touch the media area, the viewer and the inspector that 0.9.0 changed.
- [ ] **Watch party:** a watch party starts and plays while a 0.9.0 share runs, and the media area still resizes in both.

The rest of the 0.9.0 acceptance criteria are covered by the E2E suite and the engine and vitest
suites; their rows, pointers and commands are in
[`specs/07-08-bovnar-media-streams-0.9.0-acceptance-inventory.md`](../specs/07-08-bovnar-media-streams-0.9.0-acceptance-inventory.md).
Report the result per AC there, with the screenshots of section 9.
