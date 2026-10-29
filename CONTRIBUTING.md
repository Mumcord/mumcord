# Contributing to Mumcord

Thanks for helping. Mumcord is licensed under the AGPL-3.0-only. To keep the option to relicense later (for example for app-store distribution or code signing), every contribution needs a signed-off Contributor License Agreement (CLA).

## Contributor License Agreement

By contributing to Mumcord you agree to the following:

1. You grant the maintainer a perpetual, worldwide, non-exclusive, royalty-free license to use, modify, distribute and relicense your contribution.
2. You confirm that you have the right to contribute it (it is your own work, or you have permission from the owner, including your employer if relevant) and that it does not knowingly infringe anyone else's rights.
3. You keep the copyright to your contribution.
4. Your contribution is also available to everyone under the AGPL-3.0-only, like the rest of Mumcord.

This is a plain summary, not legal advice.

## How to agree

Every pull request must contain this line in its description:

```
I agree to the Mumcord CLA
```

The pull request template already has a checkbox for it. Pull requests without it are not merged.

## Guidelines

- Keep changes small and focused, one logical change per commit.
- Use Conventional Commits (`feat:`, `fix:`, `docs:`, `chore:` ...).
- Build, tests, type checks and linters must pass.
- Never include secrets or connect tests to public Mumble servers; use a local test server.
- Do not bump versions in `package.json` or `Cargo.toml`: the version comes from the release tag at build time, and a local build shows `dev~YYYYMMDD` (see [RELEASING.md](RELEASING.md#versions)).

## Dependency security and licence checks

The CI job "Dependency checks" (`.github/workflows/dependency-audit.yml`) also runs every Monday on `main`, so a new advisory against unchanged dependencies shows up without any pull request. It runs [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) with `deny.toml` for the app workspace and for `scripts/smoke/seed`, and `npm audit --omit=dev --audit-level=high` for the production npm packages. To run it locally: `cargo deny check` in the repository root and in `scripts/smoke/seed`, and the `npm audit` command above.

When it fails:

- **Advisory (security, unsound, unmaintained or yanked crate):** update the crate (`cargo update -p <crate>`). If there is no fix, or the fix needs a major upgrade of something we do not control (for example Tauri), add the advisory id to `ignore` in `deny.toml` with a `reason` and the number of a GitHub issue that tracks it, and mention it in your pull request. Never ignore an advisory without a reason and an issue.
- **Licence:** use a different crate or ask first. The `allow` list in `deny.toml` is the same as `accepted` in `about.toml` (the third-party notices); add a licence to both or to neither.
- **Source:** dependencies come from crates.io only, plus the mumbot tag pinned in `crates/engine/Cargo.toml`. A new git or registry source needs an explicit entry in `[sources]` of `deny.toml` and a reason in the pull request.
- **npm audit:** update the package (`npm update <package>`). If no fixed version exists, say so in the pull request and note the accepted finding here; `npm audit` itself has no ignore list, so the workflow step is adjusted only together with that note. No exceptions are accepted at the moment.

## Dependency updates

Dependabot (`.github/dependabot.yml`) opens one pull request per ecosystem every Monday for minor and patch updates of Cargo, npm and GitHub Actions dependencies; major updates come as separate pull requests. `tauri-specta` and `specta` are pinned to exact versions and ignored; bump them together by hand.

The third-party notices (`THIRD_PARTY_NOTICES`, `ui/public/third-party-notices.txt`) are generated automatically: after a dependency change lands on `main`, a workflow opens the pull request "chore: regenerate third-party notices", and every release regenerates them before bundling. Do not update them in your pull request. A new dependency must use an allowed licence (`accepted` in `about.toml` for Rust, `npm_allowed` in `scripts/generate-notices.sh` for npm); see [RELEASING.md](RELEASING.md#third-party-notices).
