# Roadmap

Mumcord is built epic by epic. Every epic gets a specification in this folder
(`{nr}-{name}-specification.md`) plus brainstorming, task lists and implementation notes.
The canonical, always-current overview is the
[README roadmap table](../README.md#roadmap) and the
[Roadmap wiki](https://github.com/Mumcord/mumcord/wiki/Roadmap). This file is a local index
of the specs and what has shipped.

## Epics and status

| Epic | Specs | Status |
| --- | --- | --- |
| Epic 1 — Foundation | `01-foundation-*` | Released (0.1.0–0.3.1) |
| Epic 2 — Multi-server, themes, watch party, screen sharing | `02-multi-server-themes-*` | Released (0.4.0–0.4.3) |
| Epic 3 — Comfort and polish | `03-*` | Released (0.5.0) |
| Epic 3.1 — Plugin system | `06-plugin-system-*` | Released (0.6.0) |
| Epic 4.1 — File sharing, camera, screen-share upgrade, plugin platform 1.1 | `07-file-sharing-screenshare-*`, `08-camera-sharing-*`, `09-plugin-platform-1-1-*` | Released (0.7.0) |
| Epic 4 — Admin features | `10-admin-moderation-*` | In progress (0.7.x → 0.8.0) |
| Activity log | `11-activity-log-*` | Released (0.7.2) |
| Epic 5 — Android and iOS | `05-mobile-app-*` | In progress: Android functionally complete (draft PR Mumcord/mumcord-mobile#29), iOS paused at about 55 % |
| Epic 6 — Release and distribution (store listings) | — | Later |

## Spec index

- `01-foundation-specification.md` — language, UI framework, audio and network libraries,
  target platforms, packaging. With brainstorming, tasks, implementation notes and a scale report.
- `02-multi-server-themes-specification.md` — server management and theming. With brainstorming
  and tasks.
- `03-comfort-polish-specification.md` — hotkeys, tray, whisper, Do Not Disturb and similar
  polish. With brainstorming.
- `03-identity-manager-specification.md` — identities and certificates. With tasks.
- `03-server-dialog-specification.md` — the connect server dialog. With tasks.
- `03-163-watch-together-and-screen-share-*` — watch parties and screen sharing (implementation
  and tasks).
- `03-175-mumble-link-card-tasks.md` — the Mumble Link positional-audio card.
- `04-mumpeer-relay-specification.md` — the optional Mumpeer relay for direct connections.
  With brainstorming. Lives in its own repository; Mumcord works fully without it.
- `05-mobile-app-specification.md` — the Android and iOS apps: native Kotlin and Swift apps on
  the shared Rust engine, in the repository Mumcord/mumcord-mobile. With brainstorming, tasks and
  the implementation report (`05-mobile-app-implementation.md`).
- `06-plugin-system-specification.md` — the WebAssembly plugin host, SDK, packages and registry.
  With brainstorming and tasks.
- `07-file-sharing-screenshare-specification.md` — file sharing and the screen-share upgrade.
  With brainstorming.
- `08-camera-sharing-specification.md` — camera sharing. The background effects come from the
  standalone open-source crate `bgmatte` since 0.9.1 (issue #454); it lives in its own
  repository.
- `09-plugin-platform-1-1-specification.md` — plugin API 1.1.
- `10-admin-moderation-specification.md` — moderation and server administration. With
  brainstorming. Delivered incrementally: kick/ban and ban list (0.7.0), moving people,
  moderation, channel permissions and channel management (0.7.2), registered users (0.8.0).
- `11-activity-log-specification.md` — the activity feed and notifications. With brainstorming
  and tasks.
- `12-ssd-write-reduction-specification.md` — fewer disk writes (buffered logs, batched chat
  history, durability and cache switches). Plan in `docs/ssd-write-reduction.md`.
- `13-plugin-platform-1-2-specification.md` — plugin API 1.2: theme tokens and the UI kit for
  plugin web pages (0.8.0).
- `14-hwcodec-crate-specification.md` — the video encoders and decoders as the standalone
  open-source crate `hwcodec` (0.9.0). With brainstorming and tasks. Lives in its own repository.

## Current state

Version 0.7.2 is released. `milestone/0.8.0` carries the remaining Epic 4 admin work; the
[Unreleased] section of `CHANGELOG.md` lists what has landed there so far. Features go to the
milestone branch during a milestone and to `main` as fixes and patch releases — see `CLAUDE.md`
under "Branches and releases".

Epic 5 (mobile apps), as of 2026-11-23: the engine preparation shipped with 0.9.0 and its
follow-ups (#913) are on `milestone/0.10.0`. The Android app is functionally complete and waits
in the draft pull request Mumcord/mumcord-mobile#29 for its first signed release. iOS is paused
since 2026-11-15 at about 55 % until Android is finished; the remaining scope is in
`docs/ios-backlog.md` of the mobile repository. Details: `05-mobile-app-implementation.md`.
