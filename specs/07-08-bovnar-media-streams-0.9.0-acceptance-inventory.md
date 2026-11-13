# Acceptance inventory — Bovnar media streams (0.9.0)

What task E4 (`specs/07-08-bovnar-media-streams-0.9.0-task-briefs.md`) has to report, one row per
acceptance criterion, with the evidence that already exists in this repository and the setup it
needs. The run fills the last column and reports per AC: `AC-7.72 ✅` or `❌ <symptom>`.

## Run 1 — 2026-11-12 · not executable on this machine

The run stopped at the environment. Nothing here is a product defect: the acceptance cells could
not be started. Repeated on a capable machine, unchanged.

| Blocker | What it is | What it costs |
| --- | --- | --- |
| `AVX2` | The CPU is an i7-2700K (Sandy Bridge, AVX but **no AVX2**). SVT-AV1 refuses to start: `the CPU has no AVX2, and SVT-AV1's code for such CPUs crashes` | No AV1 encode or decode, so no share and no camera can start: every live cell, every E2E media spec, every media screenshot |
| `DOCKER` | `docker` is not installed. `podman` 5.7 with `podman-compose` is, but `testenv/up.sh` and `up.sh` of the mumpeer testenv call `docker compose` | No local Mumble server, no relay |
| `SUDO` | `sudo` asks for a password and there is no TTY for it | The `netns` cells (AC-7.77 (c), AC-7.86, AC-8.38) |
| `MUMPEER` | No released mumpeer 0.3.0: upstream tags end at v0.2.1 and `main` has no media profile. The work exists only on the branch `feat/r5-media-profile` of the local checkout `/home/json/ws/mumpeer` (advertises `"media": 2`) | Every new-relay cell, and the keyframe throttle ACs, which live on the relay side |
| `V080` | No v0.8.0 build: the tag exists, no artefact, nothing built here (needs the same SVT-AV1 and, for the app, a full Tauri build) | Every AC that needs the old client |

What did run, and passed:

| Suite | Result |
| --- | --- |
| `cargo test -p mumcord-p2p --locked` | all green, including `media_relay_allocations` (3/3) and `relay` (7/7) |
| `cargo test -p mumcord-p2p --locked --test media_fuzz -- --ignored` | 4/4 |
| `cargo test -p mumcord-p2p --locked --test media_conformance` | 3/3 |
| `cargo test -p mumcord-engine --locked --test media_fuzz -- --ignored` | 6/6 |
| `cargo test -p mumcord-engine --locked` | 1097 passed, **5 failed, all five the AVX2 message above** (`screen_share::encoder::selftest`, `encoder::forced_hardware_runs_the_software_encoder_but_reports_the_gpu`, `encoder::selection_never_waits_for_a_missing_selftest`, `watch::av1_frames_round_trip_through_the_engine_decoder`, `media::picks_change_live_and_sharp_text_restarts_av1_with_screen_content`) |
| `cargo bench -p mumcord-p2p --bench media` (pinned to one core) | every row 0 allocations per message; 150 B audio 19–33 ns, 30 KB delta 20–2956 ns, 400 KB keyframe 82–61389 ns depending on the path |
| `npm test` (vitest) | 3516 tests in 319 files, all green |
| `cargo bench -p mumcord-engine --bench media_crypto` | not run: the release build of the engine did not finish in the time budget |

SVT-AV1 4.2.0 was built once for these runs: `scripts/build-svt-av1.sh ~/.cache/mumcord-build/svt-av1`,
used through `SVTAV1_STATIC_DIR`.

**Next:** task E4a (in `07-08-bovnar-media-streams-0.9.0-task-briefs.md`) has since put SVT-AV1, a
mumpeer with the media profile, a 0.8.0 build and `testenv/.env` in place on this workstation. What
is left is the machine itself: an AVX2 CPU, docker with compose and passwordless `sudo`. Run
`testenv/preflight.sh` first; the two ✅ rows need no repeat.

**Scope.** The 0.9.0 scope is AC-7.72 … AC-7.88 (spec 07 §11) and AC-8.29 … AC-8.41 (spec 08
§11) — 30 criteria. The addendum still names the range AC-7.72 … AC-7.84 and AC-8.29 … AC-8.41;
AC-7.85 … AC-7.88 were added to spec 07 under the same 0.9.0 heading and are in scope.

**Evidence kinds.** `E2E` = a WebdriverIO spec under `e2e/specs/` that drives the real app.
`UI test` = vitest under `ui/src/`. `engine test` = Rust test (unit, or an `#[ignore]`d live test
that needs `testenv/`). `manual` = needs a human, real hardware, an 0.8.0 build, `sudo`, or a
capture on the relay host. `none` = nothing in this repository covers it.

Pointers are written as `path` › test title. Every pointer in this file was opened and checked;
where the specification names a test file that does not exist, the row says so.

## How to run this pass

### Prerequisites (once, on the capable machine)

`testenv/preflight.sh` checks all of the following and exits non-zero while anything is missing
(task E4a). Run it before every attempt and put its output in the report.

- Docker with compose (the user in the `docker` group), `sqlite3`, `openssl`, Node 20+, Rust
  (stable) and the Linux build libraries of `scripts/ci/apt-install.sh`.
- `cp testenv/.env.example testenv/.env` and set `SUPERUSER_PASSWORD`.
- A mumpeer checkout at the commit CI pins (`.github/workflows/e2e.yml`, `MUMPEER_REF`; 0.3.0 or
  later for `"media": 2`), next to this repo or at `MUMPEER_DIR`:
  `git clone https://github.com/Mumcord/mumpeer ../mumpeer`.
- The previous mumpeer and bot images for relay profile `old`
  (`MUMPEER_IMAGE` / `MUMBOT_IMAGE`; see `e2e/helpers/relayenv.ts`).
- The 0.8.0 build: tag `v0.8.0` (it contains P0.5, `a2ba6e57`). Take the release artefact of that
  tag or build it from `milestone/0.8.0` with `npm run tauri build`.
- The 0.9.0 build: the CI artefacts of the E4 commit, or a local
  `npm run tauri build` (debug builds for the camera and sound checks, so the test hooks are
  live: `MUMCORD_TEST_CAMERA=1`).

### 1. Local servers

    testenv/up.sh v15                     # Mumble 1.5 on 64738, no relay, no bot
    testenv/reset.sh v15                  # back to an empty server (Lobby, Raid, AFK)

The relay, its API and the announcing bot come from the mumpeer testenv, driven by the E2E helpers
(`relayenv.startRelay(profile)`) or by hand:

    cd ../mumpeer/testenv
    MUMPEER_PROFILE=all ./up.sh           # new relay, "media": 2, broadcasts on
    MUMPEER_PROFILE=no-camera ./up.sh     # relay without camera sharing
    MUMPEER_IMAGE=… MUMBOT_IMAGE=… MUMPEER_PROFILE=old ./up.sh   # previous release

Confirm the relay's media profile before a new-format cell and record it in the report:

    secret=$(sed -n 's/^MUMPEER_API_SECRET=//p' ../mumpeer/testenv/.env)
    curl -fsS -H "Authorization: Bearer $secret" http://127.0.0.1:44760/v1/status | jq .media

`.media == 2` is a new relay; a missing field is an old relay.

### 2. E2E suite

    npm ci
    MUMPEER_DIR=../mumpeer npm run e2e                                     # whole suite
    MUMPEER_DIR=../mumpeer npm run e2e -- --spec e2e/specs/camera/media-streams.e2e.ts

The camera path matrix (`e2e/specs/camera/media-streams.e2e.ts`) switches the relay environment
from the plain `v15` server to profile `all` inside the file, so it must run as a whole file, not
filtered by `--grep`. Every spec starts from a fresh app profile; the relay environment is shared
inside one run. Sound checks: `MUMCORD_E2E_AUDIO=pipewire` (default) and `pulse` for AC-7.66.

### 3. Engine live tests (camera paths, the same cells without the UI)

    cd ../mumpeer/testenv && MUMBOT_IMAGE=… MUMPEER_PROFILE=all ./up.sh
    MUMCORD_LIVE=1 MUMCORD_TEST_CAMERA=1 MUMCORD_CAMERA_PROFILE=all \
      cargo test -p mumcord-engine --features test-backend --locked \
      --test live_camera_paths -- --ignored --test-threads=1
    MUMPEER_PROFILE=old ./up.sh
    MUMCORD_LIVE=1 MUMCORD_TEST_CAMERA=1 MUMCORD_CAMERA_PROFILE=old \
      cargo test -p mumcord-engine --features test-backend --locked \
      --test live_camera_paths -- --ignored --test-threads=1

`MUMCORD_CAMERA_PROFILE` must match what `/v1/status` says, not `MUMPEER_PROFILE`: `MUMPEER_PROFILE`
is about allowed uses, the media format comes from the relay build
(`.github/workflows/camera-live.yml`). The other live tests of this area:
`live_camera_direct`, `live_camera`, `live_camera_stop`, `live_fanout`, `live_screen_share_media`,
`live_screen_share`, `live_relay` — same pattern.

### 4. Unit, benchmark and fuzz

    cargo test -p mumcord-p2p -p mumcord-engine
    cargo test -p mumcord-p2p --locked --test media_fuzz -- --ignored --test-threads=1
    cargo test -p mumcord-engine --locked --test media_fuzz -- --ignored --test-threads=1
    taskset -c 2 cargo bench -p mumcord-p2p --bench media

The fuzz commands are the `media-fuzz` job of `.github/workflows/ci.yml`; the seed and the case
count come from `MUMCORD_FUZZ_SEED` / `MUMCORD_FUZZ_CASES`. The benchmark wants an otherwise idle
machine pinned to one core; record the figures and the 400 KB keyframe caveat in the report.

### 5. Network namespaces, captures

The namespaces belong to the mumpeer testenv, not to this repo:

    sudo ../mumpeer/testenv/netns.sh up nodirect     # no direct connection between A and B
    sudo ../mumpeer/testenv/netns.sh up noudp        # the above, plus no UDP: TCP only
    sudo -E ../mumpeer/testenv/netns.sh run a <cmd>  # start a client inside A

Screen-share and camera media on the relay host is captured on the relay container:

    docker exec -it mumpeer-testenv-mumpeer-1 tcpdump -i any -w /tmp/media.pcap port 44759

Run the `docs/packet-inspector.md` row set for the same moments and attach the export.

A lost frame is what makes a viewer ask for a keyframe; `tools/netsim` produces one without root
(`netsim ctl loss both 20`, then `reset`). For the keyframe-throttle checks, put the viewer behind
it instead of looking for a button in the app.

### 6. Screenshots

Dark and light for every changed UI surface, 1440×900, 8-bit palette PNG, into
`docs/images/screenshots/` (naming: `<surface>-<theme>.png`, see that folder's `README.md`, whose
table also needs the captions) and embedded in the PR description. Surfaces changed in 0.9.0:
camera room header and viewer list path chips, the "This camera needs Mumcord 0.9.0" tile, the
"This share needs Mumcord 0.9.0" viewer state, the relay-needs-an-update notice, the packet
inspector media rows. E2 said its screenshots would be taken in E4.

### 7. Where the report goes

The report is this file with the verdict column filled (`⬜` → `✅` or `❌ <symptom>`), committed on
the E4 branch; a failure becomes its own fix task and a new verification run, never a fix inside
the acceptance run. Screenshots go into `docs/images/screenshots/` and into the PR description.

## Spec 07 — file sharing and screen sharing

| AC | Title (short) | Evidence | Pointer | Setup | Verdict |
| --- | --- | --- | --- | --- | --- |
| AC-7.72 | New-format share: one endless `.media` document, description on channel 0, frames on layer channels; blind relay | UI test, engine test, manual | `ui/src/features/inspector/packets.test.ts` › "tells control messages, frames and the old header apart"; `crates/engine/src/screen_share/wire.rs` › `every_message_of_the_document_is_tapped_for_the_inspector`; `crates/engine/src/screen_share/broadcast/crypto.rs` › `a_track_message_round_trips_and_hides_the_timestamp_and_frame`; B watches and the relay-host capture: manual; the automated half: `e2e/specs/media/media-streams-relay.e2e.ts` › "AC-7.72 …" | new relay, two instances, relay-host capture (sudo) | ❌ live cell not run — AVX2, MUMPEER |
| AC-7.73 | Sound on channel 5; a late joiner gets the description before its first frame and starts at a keyframe | engine test | `crates/engine/src/screen_share/media.rs` › `media_description_lists_every_layer_and_the_audio_track`; `crates/engine/src/screen_share/fanout.rs` › `a_new_viewer_starts_at_a_keyframe`; `crates/engine/src/screen_share/broadcast/crypto.rs` › `a_late_joiner_gets_the_current_and_staged_keys`; the automated half: `e2e/specs/media/media-streams-relay.e2e.ts` › "AC-7.73 …" | new relay, three instances | ❌ live cell not run — AVX2, DOCKER, MUMPEER |
| AC-7.74 | A new description applies at the next keyframe only | engine test | `crates/engine/src/screen_share/wire.rs` › `a_changed_description_applies_at_the_next_keyframe`, `media_reader_drops_frames_outside_the_description` | no relay, two instances | ❌ not run — AVX2 |
| AC-7.75 | "up to 15" → channels 1–2 and half the frame rate; C keeps the full rate; back to Auto and again | engine test | `crates/engine/src/screen_share/broadcast/link.rs` › `the_frame_rate_choice_picks_the_temporal_layer` (15 fps → layer 1), `the_viewer_fps_choice_maps_to_the_relay_cap`; `crates/engine/src/screen_share/watch.rs` › `layers_are_dropped_to_fit_the_cap`; `crates/engine/src/screen_share/fanout.rs` › `a_slow_viewer_loses_layers_then_gets_them_back` | new relay, three instances | ❌ live cell not run — AVX2, MUMPEER |
| AC-7.76 | B stops watching → A rekeys, B's old key cannot decrypt, C keeps watching | engine test | `crates/engine/src/screen_share/broadcast/crypto.rs` › `a_rekey_switches_at_the_next_video_keyframe_and_retires_the_old_key`; `crates/engine/src/screen_share/broadcast/keyx.rs` › `rekey_reaches_remaining_viewers_only` | new relay, three instances | ❌ live cell not run — AVX2, MUMPEER |
| AC-7.77 | Malformed, truncated and oversized input refused on every path: (a) direct, (b) relay over QUIC, (c) TCP, (d) old-format units | engine test (a, b); manual (c, d) | `crates/engine/tests/media_fuzz.rs` (SFrame header, track opener, direct-track decoder, legacy opener, key-exchange chunk and update); `crates/p2p/tests/media_fuzz.rs`; `crates/p2p/src/media/codec.rs` › `a_message_outside_the_profile_is_dropped_and_the_stream_goes_on`, `the_writer_refuses_channels_and_sizes_outside_the_profile`, `eof_is_clean_exactly_at_a_message_boundary`, `a_split_at_every_byte_round_trips`; `crates/p2p/src/media/mod.rs` › `every_limit_boundary_is_exact`; `crates/p2p/src/media/description.rs` › `the_description_cap_is_checked_before_parsing` | no relay + new relay; (c) `netns noudp` | ❌ (a)(b) automation passes; (c) SUDO, (d) not run |
| AC-7.78 | 0.9.0 sharer, 0.8.x viewer, old relay → old format reaches the viewer | engine test, manual | `crates/engine/src/screen_share/wire.rs` › `the_format_switch_writes_both_formats`; `crates/engine/src/screen_share/broadcast/admit.rs` › `an_old_viewer_gets_a_direct_copy_or_the_needs_update_answer`; the viewer itself: manual | old relay, 0.8.0 client, 0.9.0 client | ❌ not run — V080, AVX2 |
| AC-7.79 | 0.8.x viewer on a new relay → old-format direct copy, else "This share needs Mumcord 0.9.0"; two old viewers, all 8 direct slots in use | engine test, UI test, manual | `crates/engine/src/screen_share/broadcast/admit.rs` › `an_old_viewer_gets_a_direct_copy_or_the_needs_update_answer`, `at_most_eight_direct_copies_and_the_total_within_the_limit`, `a_refused_admission_falls_back_to_a_direct_copy_unless_full`; `ui/src/features/screenshare/shareView.test.tsx` › "says a share needs a newer Mumcord and offers no picture (FR-7.99)"; `ui/src/features/screenshare/watchFailure.test.ts` | new relay, 0.8.0 clients | ❌ not run — V080, AVX2, MUMPEER |
| AC-7.80 | Old relay / no relay, 0.9.0 viewer → new-format direct `STREAM_SCREEN`, no relay session, the relay-needs-an-update text only with an old relay | engine test | `crates/engine/src/screen_share/wire.rs` › `the_format_switch_writes_both_formats`; `crates/engine/src/screen_share/broadcast/crypto.rs` › `the_direct_stream_is_clear_and_has_no_sframe_layer`; `ui/src/features/screenshare/notices/FanoutNotice.tsx` (maps `oldRelay` to `notices.reasonOldRelay`) and `fanoutNotice.test.tsx` › "names the reason when the relay exists but fan-out does not work" (the reason path, exercised with `full`); the no-relay half: `e2e/specs/media/media-streams.e2e.ts` › "AC-7.80 …" | old relay and no bot, two instances | ❌ not run — AVX2, MUMPEER |
| AC-7.81 | 0.8.x sharer and viewer on an old and on a new relay use the old broadcast; the bind names the format; no mixed format in one broadcast | manual | relay log and `/v1/status` of the mumpeer testenv; nothing in this repository covers the relay side | old and new relay, 0.8.0 clients | ❌ not run — V080, MUMPEER |
| AC-7.82 | Zero allocations per message once warm; the relay benchmark shows no regression against 0.8.0; the figures are recorded | engine test, manual (bench) | `crates/p2p/tests/media_relay_allocations.rs`; `crates/p2p/src/media/codec.rs` › `the_reader_allocates_nothing_per_message_once_warm`, `the_writer_allocates_nothing_per_message_once_warm`; `crates/p2p/benches/media.rs`, `crates/engine/benches/media_crypto.rs` | none (idle machine for the bench) | ✅ 0 allocs per message in the allocation tests and the p2p bench; engine crypto bench not measured |
| AC-7.83 | Rust output equals the C reference in both directions; every network decoder survives the fuzz budget | engine test | `crates/p2p/tests/media_conformance.rs`; `crates/p2p/tests/media_fuzz.rs`; `crates/engine/tests/media_fuzz.rs`; the CI job `media-fuzz` in `.github/workflows/ci.yml` | none | ✅ conformance 3/3, fuzz 4+6 green |
| AC-7.84 | Dark and light screenshots of every changed surface; every text in all 24 locales; keyboard and screen-reader checks | UI test, manual | `ui/src/i18n/i18n.test.ts` › "lists every language once, each with its own name", "names only fallback texts English has"; `ui/src/features/inspector/packets.test.ts`; `ui/src/features/camera/room/room.test.tsx` › "This camera needs Mumcord 0.9.0"; `ui/src/features/screenshare/shareView.test.tsx`; axe runs in `e2e/specs/app/keyboard-a11y.e2e.ts` (main views, not the media surfaces); the screenshots: manual | app only | ❌ media-surface screenshots and axe not taken — AVX2; i18n and UI tests pass |
| AC-7.85 | No relay and no bot: A and B watch over a direct `STREAM_SCREEN`; C joins mid-share; a keyframe request brings a keyframe on every video channel within 1 s | engine test | `crates/engine/src/screen_share/wire.rs` › `media_document_round_trips_frames_and_fills_the_description`, `a_late_frame_before_the_first_description_is_dropped`; `crates/engine/src/screen_share/watch.rs` › `an_unanswered_keyframe_request_is_repeated`; no live or E2E run covers the inspector and C at once; the automated half: `e2e/specs/media/media-streams.e2e.ts` › "AC-7.85 …" | no relay, no bot, three instances | ❌ not run — AVX2 |
| AC-7.86 | Link forced to TCP 443 (`netns noudp`): the new-format broadcast works over TCP, `0x01` mux chunks, a message over 64 KiB reassembles, layer choice and late join work | engine test, manual | `crates/p2p/src/relay.rs` › `tcp_units_are_cut_into_frames_and_put_back_together`; the running client: manual | `netns TCP-only` (sudo), new relay, two instances | ❌ not run — SUDO |
| AC-7.87 | Keyframe request: at most one `kf` per 2 s per subscriber and per 500 ms per broadcast, a new keyframe on channels 1–4 within 1 s, C unaffected | none | the throttle lives in mumpeer, not in this repository; only the relay log and `/v1/status` show it | new relay (sudo for the log), three instances | ❌ not run — MUMPEER |
| AC-7.88 | 0.8.x sharer with a 0.9.0 viewer on an old and on a new relay: the old-format stream as today, no format change mid-share | manual | needs an 0.8.0 sharer; the 0.9.0 viewer half reads as `tcp_units_are_cut_into_frames_and_put_back_together`'s sibling tests in `crates/engine/src/screen_share/wire.rs` › `auto_reader_accepts_a_media_and_a_legacy_sharer`, `legacy_frames_are_tapped_with_their_version` | old and new relay, 0.8.0 client, 0.9.0 client | ❌ not run — V080 |

## Spec 08 — camera sharing

| AC | Title (short) | Evidence | Pointer | Setup | Verdict |
| --- | --- | --- | --- | --- | --- |
| AC-8.29 | No relay and no bot: the tile arrives within 3 s over the direct path, header "Direct · end-to-end encrypted", viewer list says Direct, C joins later | E2E | `e2e/specs/camera/media-streams.e2e.ts` › "AC-8.29: with no relay the two 0.9.0 clients watch each other over the direct path" | no relay (plain `v15`), three instances | ❌ E2E not run — AVX2, DOCKER |
| AC-8.30 | New relay → the camera uses its broadcast, each viewer's path is Relay or Direct; old relay → 0.9.0 viewers get the direct path; `relay_only` on an old relay reaches B over the old broadcast; 25 viewers | E2E, engine test | `e2e/specs/camera/media-streams.e2e.ts` › "AC-8.30: with the new relay the camera uses its broadcast, and the viewer list shows each path"; `crates/engine/tests/live_camera_paths.rs` › `a_new_relay_carries_the_camera_to_a_09_viewer`, `an_old_relay_serves_a_09_viewer_directly`, `direct_only_runs_without_any_broadcast`, `relay_only_without_a_relay_is_unavailable`; the 25-viewer scale-up: manual | new relay and old relay, two instances; 25 viewers need scripted publishers | ❌ not run — AVX2, MUMPEER; the 25-viewer cell also lacks bcast-sim |
| AC-8.31 | A 0.8.0 viewer gets `needs` and the tile text, no video, no second `cam-join`, no `STREAM_CAMERA`, no second broadcast — with a relay and with `direct_only`; on an old relay it sees the old broadcast as today; a 0.9.0 viewer of a 0.8.x camera keeps the old path | E2E (sender half), engine test, UI test, manual | `e2e/specs/camera/media-streams.e2e.ts` › "AC-8.31: a viewer without `media-v2` gets `needs` and no stream, on the relay and with `direct_only`"; `crates/engine/tests/live_camera_paths.rs` › `a_viewer_without_the_new_format_is_refused`, `an_old_relay_serves_a_08_viewer_over_its_old_broadcast`; `crates/engine/src/camera/signal.rs` › `cam_join_carries_the_media_capability_and_needs_round_trips`, `needs_is_validated_and_carries_only_the_camera_id`; `crates/engine/src/camera/recv.rs` › `needs_shows_the_text_and_waits_for_the_next_announcement`; `ui/src/features/camera/room/room.test.tsx` (the tile text); the real 0.8.0 build: manual | new relay and old relay, 0.8.0 client | ❌ not run — V080, AVX2 |
| AC-8.32 | Direct path: `want` changes the tile height within 1 s, `KEYFRAME_REQUEST` still works, hiding/leaving/blocking/`full` close the right streams, a stale session id is refused | E2E, engine test | `e2e/specs/camera/media-streams.e2e.ts` › "AC-8.32: hiding, leaving and blocking close the viewer's direct stream; a stale session id is refused"; `crates/engine/src/camera/direct.rs` › `the_direct_path_serves_at_most_eight_viewers`, `the_ninth_direct_viewer_is_full_and_an_admitted_one_keeps_its_stream`, `the_description_names_the_camera_and_its_session`; `crates/engine/src/camera/recv.rs` › `the_direct_want_message_is_the_spec_layout`, `only_a_shown_unblocked_tile_takes_a_direct_stream` | no relay, three instances | ❌ E2E not run — AVX2, DOCKER |
| AC-8.33 | A room mixes direct and relay viewers and several senders; the relay becoming unavailable turns a relay camera off with the FR-8.24 text, then it can go on again on the direct path | E2E, engine test | `e2e/specs/camera/media-streams.e2e.ts` › "AC-8.33: a room mixes a relay camera and a direct camera of two senders"; `e2e/specs/camera/stop.e2e.ts` › "AC-8.11: the relay going down turns the camera off with its notice, without a direct attempt"; `crates/engine/tests/live_camera_paths.rs` › `without_a_relay_the_camera_runs_directly`; `crates/engine/src/camera/session.rs` › `every_cell_of_the_fallback_table_picks_the_path_per_viewer` | new relay, three instances | ❌ E2E not run — AVX2, DOCKER, MUMPEER |
| AC-8.34 | With the new relay, C enters mid-camera: the room shows A within 3 s, C gets the cached description and prologue after the key exchange, then frames from the next keyframe | engine test | `crates/engine/src/screen_share/broadcast/crypto.rs` › `a_late_joiner_gets_the_current_and_staged_keys`; `crates/engine/src/screen_share/fanout.rs` › `a_new_viewer_starts_at_a_keyframe`; no E2E (the spec names `e2e/specs/camera/media-streams.e2e.ts`, which has no such test) | new relay, three instances | ❌ not run — AVX2, MUMPEER |
| AC-8.35 | Data saver: B gets channels 1–2 and 15 fps while C keeps 30; the relay's per-viewer counters cap B at layer 1; B asks for 720 and A raises the resolution at the next keyframe | engine test | `crates/engine/src/screen_share/broadcast/link.rs` › `the_frame_rate_choice_picks_the_temporal_layer`; `crates/engine/src/screen_share/watch.rs` › `layers_are_dropped_to_fit_the_cap`; `crates/engine/src/camera/recv.rs` › `data_saver_follows_the_tile_then_the_setting`, `want_is_the_rounded_tile_height_or_720_for_best`; the relay's per-viewer counters: mumpeer; no E2E | new relay, three instances | ❌ not run — AVX2, MUMPEER |
| AC-8.36 | A keyframe request: the relay forwards at most one `kf` per 2 s per subscriber and per 500 ms per broadcast, a keyframe on channels 1–4 within 1 s, C unaffected | none | the throttle lives in mumpeer; `crates/engine/src/screen_share/watch.rs` › `an_unanswered_keyframe_request_is_repeated` covers only the client's own repeat | new relay (sudo for the log), three instances | ❌ not run — MUMPEER |
| AC-8.37 | B hides A (or leaves): A rekeys, a hook holding B's old key decrypts nothing, C keeps watching, the relay sees only channel, flags and the SFrame header | E2E, engine test | `e2e/specs/camera/stop.e2e.ts` › "AC-8.12: after C leaves, the keys C kept decrypt none of A's new frames" (the `keepOldKey` hook); `crates/engine/src/screen_share/broadcast/crypto.rs` › `a_rekey_switches_at_the_next_video_keyframe_and_retires_the_old_key`; no test of the hide case specifically | new relay, three instances | ❌ not run — AVX2, MUMPEER |
| AC-8.38 | The link forced to TCP 443 (`netns noudp`): the camera works over TCP, media as `0x01` mux chunks, late join, `want` and layer drops as over QUIC, the relay log names TCP | manual | `crates/p2p/src/relay.rs` › `tcp_units_are_cut_into_frames_and_put_back_together` covers the framing only | `netns TCP-only` (sudo), new relay, two instances | ❌ not run — SUDO |
| AC-8.39 | A test sender writes a truncated document, a description above 8 KiB, a video message above 4 MiB + 64, channels 5 and 6 on a camera stream, unknown flag bits and an unknown channel: each dropped with a reason code, no crash, a clean end at a message boundary | engine test | `crates/engine/tests/media_fuzz.rs` (the same decoders over hostile bytes); `crates/p2p/src/media/mod.rs` › `every_channel_is_classified`, `only_track_channels_have_a_message_limit`, `flags_carry_only_the_keyframe_bit`, `every_limit_boundary_is_exact`; `crates/p2p/src/media/description.rs` › `the_description_cap_is_checked_before_parsing`, `duplicate_channels_and_too_many_tracks_are_refused`; the scripted-sender half the spec names (`e2e/specs/camera/media-streams.e2e.ts`) does not exist | direct and new relay, two instances | ❌ not run — AVX2; the scripted sender does not exist |
| AC-8.40 | A 0.9.0 sharer, 0.8.x viewer, old relay: the old-format broadcast, the 0.8.x viewer sees the tile, the old header in the inspector, no direct or new-format stream | engine test, manual | `crates/engine/tests/live_camera_paths.rs` › `an_old_relay_serves_a_08_viewer_over_its_old_broadcast`; `e2e/specs/camera/media-streams.e2e.ts` › "AC-8.31 …" (`needs`, the sender half); the real 0.8.x client: manual | old relay, 0.8.0 client | ❌ not run — V080, AVX2 |
| AC-8.41 | 0.8.x sharer and 0.8.x viewer on an old and a new relay: the old path as today, the bind names the old format, no mixing, no error on either client | manual | relay log and `/v1/status`; nothing in this repository | old and new relay, 0.8.0 clients | ❌ not run — V080, MUMPEER |

## Gaps

Named, not fixed. A row is listed here when it has `none`, or weaker evidence than the AC text
demands.

- **AC-7.72 … AC-7.88 now have an E2E spec, but it has never run.**
  `e2e/specs/media/media-streams.e2e.ts` covers AC-7.80 (no-relay half) and 7.85;
  `media-streams-relay.e2e.ts` covers AC-7.72 (relay half), 7.73 (relay half) and 7.76 (visible
  half). They are separate files because a shard that starts the relay environment cannot offer the
  relay-free server the no-relay cells assert (task E4b), and each names the criteria it leaves out
  in its header. It typechecks and is sharded, but it needs an AVX2 machine with docker to run; the
  relay cells also skip themselves on the mumpeer commit CI pins, which has no media profile.
  Until it runs green once, every 0.9.0 share criterion still rests on unit tests.
- **AC-7.72, AC-7.73, AC-7.85**: the new spec does run a share end to end over the new profile, but
  it has not been executed yet (see above); nothing has proven "B watches" or a mid-share join in
  a real app.
- **AC-7.75**: no test reads the relay log's dropped channels; the layer maths is unit-tested, the
  relay's side is not in this repository.
- **AC-7.77 (c), (d)**: TCP and old-format units are unit-tested, but the four-path refusal run
  the AC describes does not exist.
- **AC-7.78, AC-7.79, AC-7.81, AC-7.88**: the 0.8.x half needs a real 0.8.0 build; only the
  sender-side decision is unit-tested.
- **AC-7.80**: the fallback to a new-format direct `STREAM_SCREEN` is unit-tested, not run.
- **AC-7.82**: the benchmark exists, but no figure is recorded anywhere in the repository — the AC
  asks for the figures and the 400 KB keyframe caveat to be written down. That happens in this
  run.
- **AC-7.84**: the 0.9.0 UI surfaces have no screenshots yet (E2 deferred them to E4), and no axe
  scan reaches them.
- **AC-7.86**: only the TCP framing is tested; the forced-TCP share is manual.
- **AC-7.87, AC-8.36**: the keyframe throttle is the relay's; this repository has no test of it
  and the local mumpeer checkout (`0.2.1`) is older than the media profile, so it cannot be
  checked offline either.
- **AC-8.31**: the 0.8.0 viewer half (tile text, no retry, no video) needs a real 0.8.0 build —
  the E2E image carries only the current build, as that spec's own header says.
- **AC-8.34, AC-8.35, AC-8.39**: spec 08 names `e2e/specs/camera/media-streams.e2e.ts` for all
  three; the file has five tests (AC-8.29, 8.30, 8.31, 8.32, 8.33) and none of these. Only the
  primitives below them are unit-tested.
- **AC-8.37**: the E2E run leaves the voice channel, the AC hides the video; the rekey is unit
  tested, the hide path in a running client is not.
- **AC-8.38, AC-8.40, AC-8.41**: manual only.
- **`bcast-sim` does not exist in this checkout.** FR-8.84 places it at `tools/bcast-sim` (spec 07
  puts it in the mumpeer testenv); `tools/` holds only `mumcord-plugin` and `netsim`. AC-8.30's
  25-viewer scale-up and AC-7.79's "all 8 direct slots in use" therefore need real clients or a
  tool that has to be confirmed in mumpeer 0.3.0.
- **`netns.sh` is not in this repository.** Specs write `testenv/netns.sh`; the script lives in
  the mumpeer testenv (`../mumpeer/testenv/netns.sh`). The run must not look for it here.
