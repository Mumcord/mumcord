# Bovnar media streams – task briefs (0.9.0)

> Source plan: `specs/07-08-bovnar-media-streams-0.9.0-plan.md`
> Spec addendum: `specs/07-08-bovnar-media-streams-0.9.0-spec-addendum.md`
> Base branch: `milestone/0.9.0`. Each task gets its own branch `feat/<id>-<slug>` from it and a
> pull request into `milestone/0.9.0`; bug fixes go to `main` as usual. Exception: P0.5 ships in
> 0.8.0 and branches from `milestone/0.8.0` before the 0.8.0 release.
>
> Usage: give one fresh agent this header plus exactly one task section. Never bundle tasks.
> The brief, the plan and the addendum are the contract; the agent asks before guessing.

## Shared rules for every agent

- Read `CLAUDE.md`, the plan and the addendum sections named in the brief. Earlier specs 07 and 08
  still apply unless the addendum overrides them.
- Own only the listed files/areas. A change outside them goes in the final report to the lead.
- The spec is the truth: MVP scope, no extras. Unclear or contradictory → ask the lead.
- Tests come with the code. Before "done": build, tests, type checks and linters pass. If not,
  say so honestly with the output.
- Everything from the network is untrusted. Validate before allocating, never log media content,
  names, chat text or keys.
- Client only: no Mumble server changes, no server plugins or config. `mumpeer` and `mumbot` are
  separate companion repos and are the only places where relay/bot changes happen.
- Never connect to public or third-party Mumble servers. Use `testenv/up.sh v15` (or the port
  base assigned by the lead) and `MUMPEER_DIR=../mumpeer` for relay E2E.
- Never commit, push, release or sign with real keys unless the lead asks. Then: small focused
  Conventional Commits, no `git add -A`, no AI attribution, no secrets.
- Delete your own build cache in `~/.cache/mumcord-build/<agent>` when done.
- Final report ≤ 10 lines: status, changed paths, contracts the next task needs, new work,
  anything open. No code dumps.

## Status and order

| ID | Title | Status | Depends on | Cx |
|---|---|---|---|---|
| P0.1 | Release 0.8.0, create `milestone/0.9.0` | in progress — waits for P0.5 merge and the 0.8.0 release (rc.3) | P0.5 | – |
| P0.2 | Bump `bovnar` to 2.1.0, notices, plugin wasm | done | – | – |
| P0.3 | Spike: bovnar 2.1.0 on static musl (mumpeer) | done — PASS, both static targets | – | complex |
| P0.4 | Spike: encoder output → temporal-layer channels | done — PASS, `channel = 1 + layer`, 4 layers | – | complex |
| P0.5 | 0.8.0: receive `needs`, tile text, 24 locales | done — `a2ba6e57`, PR #728 CI green; open before merge: screenshots, bindings regen, 4 locale reviews | – | simple |
| S1 | Finalize the spec addendum, resolve D1–D14 | done — merged into specs 07/08 (`bb7e06a6`) | P0.4 | simple |
| S2 | Final acceptance criteria for every fallback cell | done — `3cd26e2d`; uniform-shape fix `407344e1` unpushed | S1 | simple |
| W1 | `media` module: channels, control kinds, description, limits | done — `1b8851ad`, branch not pushed | S1 | complex |
| W2 | `MediaWriter` / `MediaReader` over `bovnar::codec` | done — `2675531e` incl. interleave fix, branch not pushed | W1 | complex |
| W3 | SFrame crypto profile for the media stream | done — `262c9d2e`, branch not pushed | W1 | complex |
| W4 | Conformance vectors and fuzzing | done — `33171c65`, pushed | W2, W3 | complex |
| W5 | Benchmark regression against today's header | done — `962c2048`, pushed | W2, W3 | simple |
| W6 | Legacy header behind a `legacy` module | done — `f4a8516d`, branch not pushed | S1 | simple |
| D1 | Direct `STREAM_SCREEN` format switch | done — `04026690`, branch not pushed | W2, W3, S2 | complex |
| D2 | Sharer: description, layer routing, format choice | done — `a950bec9`, branch not pushed | D1 | complex |
| D3 | Viewer: description, changes at keyframes, both formats | done — `b41444ab`, branch not pushed | D2 | complex |
| D4 | Capability negotiation and refusal texts | done — `12bb9696`, `abc88dd0`, `7794576b`, `b103d57c`; 24 locales in E2 | D3 | complex |
| D5 | Direct camera path (`STREAM_CAMERA`) | open | D4 | complex |
| D6 | Camera relay/direct choice and fallback matrix | open | D5, P0.5 | complex |
| D7 | Watch-party UI regression | open | D1 | simple |
| R1 | Relay broadcast profile types and new links | done — `f8d35244`, branch not pushed separately (in `feat/r1-profile-types`) | W1, W2, W6 | complex |
| R2 | Relay transports: QUIC streams and TCP mux chunks | done — `dff7deb9`, `feat/r2-transports` | R1 | complex |
| R3 | Engine broadcast link: media profile end to end | done — `c27812f4`, `2f317177`; E2E still needs R5 | R1, W3, R5 | complex |
| R4 | Plugin relay data on channel 6 | done — `875d1656`, `feat/r4-plugin-channel6` | R1 | complex |
| R5 | mumpeer: new format, routing, description cache | done — `4b94be6` on `sothis/mumpeer`, PR #17 | P0.3, W1, W2 | complex |
| R6 | mumbot: pass `mv` through the broadcast messages | done — `0887674` on `sothis/mumbot`, PR #72 | S1, R1 | simple |
| E1 | Packet inspector: description, channels, format | open | D1, R2 | complex |
| E2 | UI texts and 24 locales for fallback paths | partial — `8f8a516b`: screen-share fallbacks in 24 locales; camera texts wait for D6 | D4, D6 | simple |
| E3 | Notices and licences for Bovnar 2.1.0 | open | – | simple |
| E4 | Acceptance run of every AC | open | all (incl. P0.5) | complex |
| E5 | Release Mumcord 0.9.0, mumpeer 0.3.0, mumbot | open | E4 | simple |

Next steps: merge P0.5 (PR #728) after its screenshots, bindings regeneration and locale review,
then release 0.8.0 (P0.1). Push or merge the W1–W3 and W6 branches. D1–D4 and R1–R6 are done;
E2 has the screen-share fallbacks and needs the camera texts after D6. D5/D6 and E4 remain. The
engine dev-dependency temporarily points at `sothis/mumbot` branch `feat/media-v2` (PR #72) until
the 0.9.0 tag exists. E3 can run at any time.

---

## P0.3 — Spike: Bovnar 2.1.0 on static musl (mumpeer)

**Type:** complex · **Repo:** `Mumcord/mumpeer` (separate; not in this workspace)

**Goal:** prove that mumpeer can build the C part of `bovnar` 2.1.0 for its static musl targets
with cargo-zigbuild, so R5 does not start on a broken base.

- Clone mumpeer next to this repo (`../mumpeer`, or the path the lead gives) and read its CI for
  the exact targets and zigbuild flags. Do not modify Mumcord.
- Bump only the spike branch to `bovnar = "2.1.0"`, build the static binary for each release
  target, and run it (`--version` or its testenv entry point).
- If the C build fails, report the exact error and the smallest upstream change needed (for
  example a `codec-only` feature). Do not work around it by dropping the C library silently.
- Leave no production changes behind unless the lead asks for a follow-up task.

**Done when:** a static binary builds and runs on every target; the report lists commands,
targets, sizes and any failure. A failed spike is a valid result if it names the blocker.

---

## P0.4 — Spike: encoder output to temporal-layer channels

**Type:** complex · **Owns:** `crates/screenshare/` and encoder tests under
`crates/engine/src/screen_share/encoder/` (read the rest)

**Goal:** settle D3 — can every encoded frame be routed cleanly to channel `1 + temporal_layer`?

- Audit every encoder path the share and camera use: VP9 L1T3 (`crates/screenshare/src/encoder.rs`
  exposes `Frame::temporal_layer`), AV1 SVT (`av1_svt.rs`), hardware QSV/VAAPI and the camera
  pipelines. Check keyframes, layer switches and the 4-layer cap.
- Write a test or a short benchmark that shows, for each path, that every frame has a layer and
  that a keyframe is always layer 0 (or explain why it is not).
- If a path cannot attribute a layer before encoding, report the evidence and the fallback the
  addendum allows (layer in the flags byte or a short prefix read by the relay).
- Do not edit the spec; S1 records the result.

**Done when:** every path is classified clean/not clean with a reproducible test or measurement,
and the report gives the result S1 needs.

---

## P0.5 — 0.8.0 forward-compatibility: `needs` receive path

**Type:** simple · **Branch:** `milestone/0.8.0` (0.8.0 is unreleased) · **Owns:**
`crates/engine/src/camera/signal.rs`, `crates/engine/src/camera/recv.rs`, the camera tile in
`ui/src/features/camera/room/` and `ui/src/locales/*/camera.json`

**Goal:** a 0.9.0 sender cannot serve a 0.8.x viewer of a new-format camera. It tells the viewer
with the new `mumcord.camera` message `needs`; 0.8.0 must show the clear text and stop retrying,
so the user sees a reason instead of "Couldn't join …" forever.

- Protocol (fixed for 0.8.0 and 0.9.0, spec 08 §6.5): a `mumcord.camera` Bovnar document
  `.v = 1; .t = "needs"; .id = <16 lowercase hex camera session id>`, sender → viewer. Parse it
  like the other camera messages: size limit checked before decoding, unknown fields ignored,
  malformed documents refused without a crash or payload content in the log.
- On `needs` for the camera session named by `id`: enter a new "needs update" tile state that
  shows "This camera needs Mumcord 0.9.0", send no further `cam-join` for that camera and do not
  enter the FR-8.52 10 s retry. A `needs` for another `id` and an unknown `t` are ignored. The
  camera's next `on` clears the state and joins normally.
- Add the text to `en` and all 24 locales next to `tile.failed`; keep "Couldn't join {name}'s
  camera" for a real join timeout (no `needs`).
- No other 0.8.0 camera behaviour changes, no 0.9.0 sender code here, and nothing is changed on
  `milestone/0.9.0`.

**Done when:** unit tests cover the parser (valid, malformed, oversized, wrong `id`), the tile
state and the no-retry rule; a 0.8.0 build shows "This camera needs Mumcord 0.9.0" when a fixture
or test CLI sends `needs`, does not retry, and joins again on the next `on`; no untranslated text.

---

## S1 — Finalize the spec addendum and resolve D1–D14

**Type:** simple · **Owns:** the addendum file and the two specification files

**Goal:** turn the committed draft into the reviewed specification that code may follow.

- Fold the addendum into `specs/07-file-sharing-screenshare-specification.md` and
  `specs/08-camera-sharing-specification.md` with the `/spec` skill on `milestone/0.9.0`.
- Resolve D1–D14 from plan §6. Already decided: D12 camera relay-first, D13 direct limit 8,
  D14 old format through 0.9.x. The product-owner answers are in plan §12; P0.4 settles D3.
  D5 (`want`: reverse-stream message or `mumcord.camera` signalling) must be picked, not left open.
- Keep FR/AC numbers provisional while 0.8.0 is unreleased; renumber after its merge.
- The addendum's "Open points" section must not contradict the decisions table.

**Done when:** the addendum is folded into specs 07/08, no open point remains unanswered except
the provisional numbering, and the product owner has reviewed it. `/spec` commits it.

---

## S2 — Final acceptance criteria

**Type:** simple · **Depends:** S1 · **Owns:** specs 07/08 and the addendum

**Goal:** every acceptance criterion is checkable in the running client against a local server and
a local mumpeer.

- Cover every cell of the fallback matrix for screen share and camera, old clients and old relays,
  late join, layer switch, keyframe request, rekey, TCP fallback, and malformed/truncated/oversized
  input.
- Each AC names the setup (clients, relay profile, server) and the observable result.
- Add the matching E2E spec names where the existing `e2e/README.md` relay environment can run
  them; flag the ones that need a manual listening check.
- No AC may depend on a public server or on server-side changes.

**Done when:** every row of the fallback matrix and every failure path has an AC that E4 can run
without interpreting the spec.

---

## W1 — `media` module: channels, control kinds, description, limits

**Type:** complex · **Depends:** S1 · **Owns:** `crates/p2p/src/media.rs` (or `media/`) and the
module line in `crates/p2p/src/lib.rs`

**Goal:** the single place that names the new wire profile (FR-7.90, FR-7.92–7.95).

- Channel constants 0 control, 1–4 layers (`channel = 1 + layer`), 5 audio, 6 plugin data,
  7–4095 reserved. Control kinds 0 description, 1 `keyreq`, 2 `key`, 3 `kf`, 4 `layers`,
  5 `want`, 6 `gone`.
- Description parser/writer with `bovnar::DomDoc` (the same crate pattern as
  `crates/engine/src/camera/signal.rs` and `crates/engine/src/files/wire.rs`), fields
  `.v`, `.timebase`, `.kind`, `.tracks[]`; unknown fields ignored; 8 KiB cap checked before parse.
- Limits: video 4 MiB + 64, audio 4 KiB + 64, plugin data 64 KiB + 64, channels 0–4095.
- Typed errors; no panics; no media content in errors or logs.

**Done when:** unit tests cover description round-trip, unknown fields, oversized input, every
channel and control kind, every limit, and a malformed document.

---

## W2 — `MediaWriter` / `MediaReader` over `bovnar::codec`

**Type:** complex · **Depends:** W1 · **Owns:** `crates/p2p/src/media*`

**Goal:** the incremental endless-document codec of FR-7.89/7.94.

- Use `bovnar::codec` (`MuxWriter`, `MuxReader`, peek/skip/forward) from the pinned 2.1.0; do not
  copy the closed AGPL prototype #682.
- Write `.media = \x00 …messages… \x00;`. A message is `varint(channel) varint(length) bytes`;
  every message starts a new chunk; a message above 64 KiB spans chunks that each start with the
  channel varint; the writer ends only when the link closes.
- Reader accepts arbitrary byte splits, returns borrowed payload slices, treats EOF at a message
  boundary as a clean stop and reports malformed streams without allocating on them.
- Zero allocations per message once warm, proven with the counting allocator.

**Done when:** round-trip, split at every byte, > 64 KiB, terminator, malformed input, reset and
the zero-allocation test pass.

---

## W3 — SFrame crypto profile for the media stream

**Type:** complex · **Depends:** W1 · **Owns:** `crates/engine/src/screen_share/broadcast/crypto.rs`
and `keyx.rs` only if its interface changes

**Goal:** FR-7.92/7.95: the relay sees only channel, flags and the SFrame header.

- Keep the existing RFC 9605 key schedule and the key exchange; do not invent crypto.
- On the relay path put a real SFrame header (compact key id and counter) in the message; the AAD
  is `channel varint + flags + SFrame header`; the 8-byte timestamp moves inside the ciphertext.
- Rekey at a keyframe as today; a viewer that leaves gets no new key; older keys are retired.
- Direct stream: no SFrame layer (QUIC TLS already protects it); timestamp is clear.
- No nonce reuse, no key or media content in errors/logs.

**Done when:** RFC 9605 vectors, AAD tampering, rekey, late-key and old-key-retirement tests pass;
the existing sealer/opener tests still pass adapted.

---

## W4 — Conformance vectors and fuzzing

**Type:** complex · **Depends:** W2, W3 · **Owns:** new fuzz targets, their CI wiring, conformance
test files (coordinate CI file ownership with the lead)

**Goal:** the Rust codec cannot drift from the C reference and every network decoder survives
hostile input.

- Shared vectors through `bovnar` 2.1.0: Rust output must be byte-identical to the C
  `bvnr_mux_send`; the C demux must read Rust output and vice versa.
- Fuzz every decoder that reads from the network: media reader, description, control messages,
  crypto opener and the legacy header. Add a fixed CI budget (the repo has no fuzz targets yet;
  cargo-fuzz or a bounded property harness — propose the smallest one that CI can run).
- Failures must be reproducible from the corpus.

**Done when:** vectors pass both directions and the fuzz job runs clean for its budget in CI.

---

## W5 — Benchmark regression against today's header

**Type:** simple · **Depends:** W2, W3 · **Owns:** benchmark files only

**Goal:** record the real numbers for AC-7.82.

- Reimplement the benchmark from the addendum (do not copy #682): 150 B audio, 30 KB video delta
  and 400 KB keyframe; write, relay peek/skip/forward and full read against today's 14/21-byte
  header.
- Include allocations per packet and the accepted 400 KB keyframe caveat.
- Record figures in the PR; no production behaviour change.

**Done when:** figures are reproducible and recorded, and no target is missed beyond the accepted
keyframe caveat.

---

## W6 — Legacy header behind a `legacy` module

**Type:** simple · **Depends:** S1 · **Owns:** `crates/p2p/src/broadcast.rs` (or its new submodule)

**Goal:** FR-7.98: the old format stays available for fallback and is clearly marked legacy.

- Move the old `Header` / `Unit` / `Content` wire types behind a `legacy` module without changing
  a byte of the format; keep every existing test.
- No new code uses it by default; the fallback selection is the only caller after D1/R1.
- Coordinate with R1: it adds the new profile types next to these; land W6 before R1 starts.

**Done when:** the old format round-trips and its existing tests pass unchanged.

---

## D1 — Direct `STREAM_SCREEN` format switch

**Type:** complex · **Depends:** W2, W3, S2 · **Owns:** `crates/engine/src/screen_share/wire.rs`
and the read/write call sites in that module

**Goal:** one place that switches between the new document and the old v1/v2 frames.

- New profile for `media-v2` peers, old v1/v2 for old peers; keep `Frame` / `FrameHeader` as the
  internal type.
- Fill width, height, codec and timestamp from the description/flags instead of the old header.
- Keep the old code path covered by its tests; a bad peer format never becomes a broken picture.
- Use the W1/W2 types; do not duplicate channel or limit constants.

**Done when:** screen-share unit tests pass for both formats and the old header stays covered.

---

## D2 — Sharer: description, layer routing, format choice

**Type:** complex · **Depends:** D1 · **Owns:** `crates/engine/src/screen_share/start.rs`,
`quality.rs`, `signal.rs`, `mod.rs`

**Goal:** the sharer half of the profile.

- Write the description at stream start and on every quality/codec/resolution change; route each
  frame to `1 + temporal_layer`; map direct keyframe requests; choose the viewer's format from the
  hello/`Watch` capability.
- A description is applied by the viewer at a keyframe; resend at every rekey.
- Run the P0.4 result if it narrowed D3; do not invent a second mapping.
- No allocation in the per-frame path beyond today's pipeline.

**Done when:** a direct share works between 0.9.0 clients and with a 0.8.x viewer at every quality
step and codec, with unit tests for the switch and the routing.

---

## D3 — Viewer: description, changes at keyframes, both formats

**Type:** complex · **Depends:** D2 · **Owns:** `crates/engine/src/screen_share/watch.rs`,
`media.rs` viewer side

**Goal:** the viewer half of the profile.

- Read the description before the first frame; apply a new one only at a keyframe; request
  keyframes on the reverse stream; treat EOF at a message boundary as a clean stop; accept both
  formats.
- A late frame or a mid-stream quality change must never be decoded with the wrong description.
- No media content in logs; malformed input drops with a reason code.

**Done when:** direct viewer tests cover a late frame, a mid-stream quality change, an old sharer
and a truncated stream.

---

## D4 — Capability negotiation and refusal texts

**Type:** complex · **Depends:** D3 · **Owns:** `crates/engine/src/mumcord.rs`,
`crates/p2p/src/peer.rs`, `crates/engine/src/peer.rs`

**Goal:** FR-7.96/7.99: the new format is offered only when both ends and the relay support it.

- Hello feature `media-v2`; `Watch` optional `mv: 2`; relay announcement `mv`; `bcast-req` `mv`;
  `bcast` and `bcast-tok` carry it; relay `bind` optional `"media": 2`.
- A missing field means the old format. Never offer the new format to a viewer that cannot read
  it and never send a broken picture.
- Refusal texts: "This share needs Mumcord 0.9.0", "This server's relay needs an update";
  keep "This share is full".
- Unknown fields stay ignored; old clients keep working.

**Done when:** old-client and old-relay scenarios show the clear message and no broken picture;
unit tests cover every missing/garbled capability combination.

---

## D5 — Direct camera path (`STREAM_CAMERA`)

**Type:** complex · **Depends:** D4 · **Owns:** `crates/engine/src/p2p.rs` (new kind),
`crates/engine/src/camera/*`

**Goal:** FR-8.92/8.93: camera works without a relay and with an old relay.

- Add `STREAM_CAMERA = 5` next to the existing stream kinds; one stream per admitted viewer,
  opened by the sharer after `cam-join`; refuse a stale camera session id.
- When a `cam-join` carries no `mv: 2` (the viewer's hello lacks `media-v2`) and the camera uses
  the new format, send `mumcord.camera` `needs` (`v`, `id`, spec 08 §6.5) to that viewer and open
  no `STREAM_CAMERA`; there is no old-format direct camera copy for 0.8.x.
- Reuse the media document/description with video channels only; the description carries the
  camera session id.
- Reverse stream: keep `KEYFRAME_REQUEST = 1`; add `WANT = 2 + u16 height` (or the signalling
  choice S1 fixed); apply `want` within 1 s.
- Keep admission, same-channel, blocked users, viewer limit 8, `full` and rekey rules as today.
- The camera room may mix direct and relay viewers and several senders.

**Done when:** camera works with no relay, with an old relay and between two 0.9.0 clients; a
viewer without `media-v2` gets `needs` and no stream; the camera room still passes its tests.

---

## D6 — Camera relay/direct choice and fallback matrix

**Type:** complex · **Depends:** D5, P0.5 · **Owns:** `crates/engine/src/camera/session.rs`,
`avail.rs`, `signal.rs`, `crates/engine/src/p2p.rs` (path choice)

**Goal:** FR-8.94/8.95 and the product-owner decisions.

- Relay broadcast first when a new relay is available; direct when the relay is old, missing or
  the user is in direct-only mode. The viewer list shows the path per viewer.
- The sender sends `mumcord.camera` `needs` (`v`, `id`, spec 08 §6.5) to a viewer whose hello
  lacks `media-v2` when the camera uses the new format; that 0.8.x viewer shows "This camera needs
  Mumcord 0.9.0" on the tile and stops retrying until the next `on`. Never open an old-format
  direct camera copy and never create a second relay broadcast for it (0.8.x has no direct camera
  receive path). Verify the text in a real 0.8.0 build that includes P0.5; if that build lacks
  the state, report it instead of inventing a workaround.
- On an old relay the camera's old-format relay broadcast serves 0.8.x viewers as today.
- A 0.9.0 viewer of a 0.8.x camera uses the old relay path as today.
- Every cell of the camera fallback matrix is tested, including `full`, leaving and blocking.

**Done when:** every camera fallback cell passes, a real 0.8.0 build shows the "needs 0.9.0"
text after `needs` with no retry and no video arrives, and the UI shows the correct path per
viewer.

---

## D7 — Watch-party UI regression

**Type:** simple · **Depends:** D1 · **Owns:** none (read-only unless a regression needs a fix)

**Goal:** the shared media view still plays and resizes after the format switch.

- Run the existing screen-share/watch-party UI tests unchanged; check the main states in the
  running client.
- A needed fix becomes a new task, not a scope extension here.

**Done when:** the existing tests pass unchanged and the report names any deviation.

---

## R1 — Relay broadcast profile types and new links

**Type:** complex · **Depends:** W1, W2, W6 · **Owns:** `crates/p2p/src/broadcast.rs`

**Goal:** publisher and subscriber speak the new document on new links; old links keep today's
framing.

- Add profile types next to the legacy `Header` / `Unit` / `Content`; new links use channel 0 for
  control/description and track channels for media; key exchange and admission stay unchanged.
- No old 14-byte header on a new link; no new header on an old link.
- Keep the old tests and add new-link tests.

**Done when:** publisher/subscriber unit tests pass for both link types and a new link never emits
the old header.

---

## R2 — Relay transports: QUIC streams and TCP mux chunks

**Type:** complex · **Depends:** R1 · **Owns:** `crates/p2p/src/relay.rs`

**Goal:** FR-7.89 transports.

- New links: one QUIC unidirectional stream per message, in order; over TCP, mux chunks inside the
  existing `0x01` frames.
- Old links keep the current framing; `bind` carries optional `"media": 2`.
- Messages cut at every byte must reassemble; a chunk-boundary drop must leave a valid stream.
- Keep the existing outer TLS/QUIC behaviour and the 1200-byte inner MTU.

**Done when:** TCP and QUIC transport tests pass for both formats and every split point.

---

## R3 — Engine broadcast link: media profile end to end

**Type:** complex · **Depends:** R1, W3, R5 (for E2E) · **Owns:**
`crates/engine/src/screen_share/broadcast/link.rs`

**Goal:** seal/open track messages, description handling, late joiner, layer filter, `want`,
plugin data channel and format selection.

- Use W1/W2/W3 types; do not duplicate constants.
- Late joiner: description and prologue first, then frames from the next keyframe, after keys.
- Layer filter drops higher channels whole at chunk boundaries; `want` changes the tile height.
- Plugin relay messages still arrive in order and once; the relay never parses the payload.

**Done when:** broadcast link tests pass, including late join, layer switch, `want` and plugin
ordering; the E2E relay path runs against R5.

---

## R4 — Plugin relay data on channel 6

**Type:** complex · **Depends:** R1 · **Owns:** `crates/engine/src/plugin_relay/broadcast.rs`

**Goal:** FR-7.93: plugin relay data uses channel 6 on new links.

- New links use channel 6 with the existing track layout; old links keep `Content::Data`.
- The relay forwards channel 6 without looking inside and treats it as lowest priority.
- Existing plugin relay tests pass for both formats; ordering and once-only delivery stay.

**Done when:** both formats pass the existing plugin relay tests and the relay never parses the
payload.

---

## R5 — mumpeer: new format, routing, description cache

**Type:** complex · **Depends:** P0.3, W1, W2 · **Repo:** `Mumcord/mumpeer` (separate)

**Goal:** the relay speaks both formats and advertises `"media": 2`.

- Add `bovnar` 2.1.0 using P0.3's spike result. New links: mux messages, channel-based layer
  filtering, chunk-boundary drops, description cache (latest kind-0 ciphertext plus prologue,
  capped at 8 KiB per broadcast), `media` bind gate, `/v1/status` `"media": 2`.
- Old links keep the 14-byte header. A new relay serves both old and new clients; a new relay
  never mixes formats inside one broadcast.
- Keep the relay blind to payloads and media content.
- Run mumpeer's live tests and `bcast-sim` for old and new clients; record a relay-cost benchmark.

**Done when:** both formats pass the live tests and `bcast-sim`, the status advertises `media: 2`,
and the benchmark is reported.

---

## R6 — mumbot: pass `mv` through the broadcast messages

**Type:** simple · **Depends:** S1, R1 · **Repo:** `Mumcord/mumbot` (separate)

**Goal:** the bot carries the capability without parsing media.

- Pass `mv` through `relay`, `bcast-req`, `bcast` and `bcast-tok`; bump `mumcord-p2p` for the
  message types; no media parsing, no payload inspection.
- Keep old clients and old relays working (missing `mv` means the old format).
- Run mumbot's live peer tests against old and new clients and relays.

**Done when:** mumbot's tests pass for both formats and the bot never looks inside media units.

---

## E1 — Packet inspector: description, channels, format

**Type:** complex · **Depends:** D1, R2 · **Owns:** `crates/engine/src/inspector.rs`,
`ui/src/features/inspector/*`, inspector docs

**Goal:** FR-7.101: make the new stream readable in the inspector.

- Show the detected format, the decoded description, and per message the channel, flags,
  keyframe state and size.
- Never log or display media content; keep the ring-buffer and capture rules.
- Update the inspector docs and any screenshots that show the view.

**Done when:** a captured share shows one endless `.media` document, the description on channel 0
and each frame on its layer channel; docs updated.

---

## E2 — UI texts and 24 locales for fallback paths

**Type:** simple · **Depends:** D4, D6 · **Owns:** `ui/src/locales/*/{screenshare,screenviewer,camera}.json`
and the components that show them

**Goal:** every new fallback and old-relay message is translated and accessible.

- Add the texts from FR-7.99/FR-8.95 and the camera path labels to `en` and all 24 locales; no
  untranslated string.
- Keyboard operability, screen-reader labels, dark/light themes and system scaling.
- Screenshots to `docs/images/screenshots/` for changed UI; embed in the PR.

**Done when:** no untranslated text, and the keyboard/screen-reader/theme checks pass.

---

## E3 — Notices and licences for Bovnar 2.1.0

**Type:** simple · **Depends:** – · **Owns:** `THIRD_PARTY_NOTICES`, `ui/public/third-party-notices.*`,
the notices scripts if needed

**Goal:** the notices gate keeps passing with the 2.1.0 provenance after all dependency changes.

- Regenerate with `scripts/generate-notices.sh` / `scripts/notices.js`; the crate is MIT and its
  `PROVENANCE.json` must match the published 2.1.0 files and checksum.
- Recheck the plugin-SDK bundled wasm library and its licence/notices files.

**Done when:** `scripts/ci/test-notices.sh` passes and the notices name `v2.1.0` with its
provenance.

---

## E4 — Acceptance run of every AC

**Type:** complex · **Depends:** all, incl. P0.5 · **Owns:** `e2e/specs/*`, test fixtures, screenshots; report
file

**Goal:** prove the whole addendum in the running client.

- Run every AC against a local Mumble server (`testenv/up.sh v15`), a local mumpeer
  (`MUMPEER_DIR=../mumpeer`) and a local mumbot. Cover every fallback combination, direct and
  relay, late join, layer switch, keyframe, rekey, TCP fallback, malformed/truncated/oversized
  input and an old client served by fallback: screen share with the old-format direct copy, and
  camera with the "This camera needs Mumcord 0.9.0" text for a 0.8.x viewer and never an
  old-format direct camera copy or a second relay broadcast.
- For the 0.8.x side use a real 0.8.0 build that includes P0.5: the sender's inspector shows the
  `needs` message to that viewer, the 0.8.0 tile shows the text, the viewer sends no further
  `cam-join` until the camera's next `on`, and no video arrives.
- Report per AC: `AC-7.72 ✅` or `❌ <symptom>`; mark manual listening checks.
- Dark and light screenshots for every changed UI surface.
- A failure becomes a fix task, then a new verification run; never fix and verify in the same
  agent.

**Done when:** every AC passes or has a named, agreed exception with the reason.

---

## E5 — Release Mumcord 0.9.0, mumpeer 0.3.0, mumbot

**Type:** simple · **Depends:** E4 · **Owns:** release metadata only

**Goal:** ship the three together with notes that explain the change.

- Follow `RELEASING.md` and the branch rules: the 0.9.0 milestone merges to `main` only when the
  whole milestone is done; tags come from `main`.
- mumpeer 0.3.0 and the mumbot patch can ship before Mumcord; old clients keep working.
- Notes: new format with 0.9.0 peers and a new relay, old format as fallback, camera without a
  relay, and the recommendation to update the relay for fan-out.
- Outward-facing: only with the user's explicit approval.

**Done when:** the three releases are tagged and the notes are published.
