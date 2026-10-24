# Mumcord

Mumcord is a modern, Discord-style desktop voice chat client for gaming groups and online communities. It is built with Rust, Tauri 2 and React/TypeScript.

Mumcord is **compatible with Mumble**. It is **not affiliated with or endorsed by the Mumble project**. "Mumble" is the name of that project; Mumcord does not use the Mumble logo.

## What it is

- A pure client: it speaks the Mumble protocol and works with unmodified, standard Mumble servers. Nothing needs to be installed or changed on the server.
- Mumble servers 1.4 and newer are fully supported. Older servers are supported on a best-effort basis; features they lack are hidden.

## Status

Early development (Epic 1). Expect missing features and rough edges. See `specs/01-foundation-specification.md` for the scope.

Test builds for Windows and macOS are unsigned. The operating system will show a security warning when you start them.

## Development setup

    npm ci
    npm run tauri dev

Other useful commands: `npm run build`, `npm test`, `npm run lint`, `npm run typecheck`.

Local Mumble servers for manual and automated tests are described in [`testenv/README.md`](testenv/README.md). Do not test against public servers.

## Licenses

Mumcord is released under the [MIT License](LICENSE). Third-party licenses, including Mumble's BSD license for the protocol definitions in `proto/`, are listed in [`THIRD_PARTY_NOTICES`](THIRD_PARTY_NOTICES). Regenerate it with `scripts/generate-notices.sh` when dependencies change.
