# Contributing to Mumcord

Thanks for helping. Mumcord is licensed under the AGPL-3.0-only and is also offered under a commercial license (see [COMMERCIAL.md](COMMERCIAL.md)). To make that possible, every contribution needs a signed-off Contributor License Agreement (CLA).

## Contributor License Agreement

By contributing to Mumcord you agree to the following:

1. You grant the maintainer a perpetual, worldwide, non-exclusive, royalty-free license to use, modify, distribute and relicense your contribution, including under commercial terms.
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
