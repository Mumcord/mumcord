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

## Dependency updates

Dependabot (`.github/dependabot.yml`) opens one pull request per ecosystem every Monday for minor and patch updates of Cargo, npm and GitHub Actions dependencies; major updates come as separate pull requests. `tauri-specta` and `specta` are pinned to exact versions and ignored; bump them together by hand.

The third-party notices (`THIRD_PARTY_NOTICES`, `ui/public/third-party-notices.txt`) are generated automatically: after a dependency change lands on `main`, a workflow opens the pull request "chore: regenerate third-party notices", and every release regenerates them before bundling. Do not update them in your pull request. A new dependency must use an allowed licence (`accepted` in `about.toml` for Rust, `npm_allowed` in `scripts/generate-notices.sh` for npm); see [RELEASING.md](RELEASING.md#third-party-notices).
