# Mumcord – modern Mumble client

Mumcord is a modern voice chat client that speaks the Mumble protocol and stays compatible with standard Mumble servers (murmur / mumble-server). Overview: `specs/roadmap.md` (once it exists). Each epic gets `specs/{nr}-{name}-specification.md`; specs are created with the `/spec` skill and built with `/dev`.
The user is a product manager and not a technical person → make technology decisions with a clear recommendation, explained simply.
Everything is in English: chat, specs, docs, code, comments, commit messages and UI text.

## Answer style: caveman mode (always)

- Chat answers extremely short: no filler, no intro, no repetition. Sentence fragments are fine.
- Only the result and next steps. Bullet points instead of prose. For code changes, `file:line` is enough instead of code excerpts.
- Must still be understandable for a non-technical person.
- **Applies to chat only.** Specs, brainstorming files, docs, commit messages and code comments are written fully and cleanly.

## Context budget: only 200k tokens → be frugal

- Search before reading (Grep/Glob). Read large files only in parts (`offset`/`limit`).
- Never read anything twice. Don't re-read your own changes to check them.
- Delegate broad searches across many files to a subagent (Explore) and take only its result.
- Keep agents short: concise brief, smallest suitable model, final report ≤ 10 lines. No long-lived agents, no status pings.
- Limit command output: `--quiet`, `| tail -n 50`, for tests print only the failures.
- Don't copy file contents, logs or diffs into the chat.
- Run independent tool calls in parallel. No unnecessary interim reports.

## Way of working

- Specs are the truth: only build what the specification says. Unclear or contradictory → ask instead of guessing. No scope creep.
- One task = small, verifiable steps. Every epic must be verifiable in the running client against a local test server (acceptance criteria of the spec).
- One fresh agent per subtask with a self-contained brief. Never bundle several tasks into one agent. The agent ends after its final report; follow-up work gets a new agent.
- Commit only on request (exception: `/spec` commits the finished spec). Never `git add -A`. Never push without being asked.
- Commits are small and focused: one logical change per commit, Conventional Commits style (`feat:`, `fix:`, `docs:`, `chore:` …), clear English message. Never add AI attribution: no `Co-Authored-By` trailers, no "Generated with" lines, no mention of Claude or AI.
- Secrets never go into the repo (no passwords, signing keys or private certificates in files, docs or examples). Use only local env files that are git-ignored and CI variables.
- Never connect to public or third-party Mumble servers for testing; use a local test server.
- **Client only.** Never build, require or modify anything on the server side (no companion services, server plugins or server config changes). Every feature must work against an unmodified standard Mumble server.

## Stack

Not decided yet – the first spec (foundation) sets language, UI framework, audio and network libraries, target platforms and packaging. Then record it here.
Don't introduce anything without a reason.

## Coding rules

**General**
- Simple over clever. No speculative abstractions. No dead code, no commented-out blocks.
- Comments explain the *why*, not the *what*.
- Follow the style of the surrounding code.
- Never swallow errors silently: log them or report them cleanly to the user.
- Strict typing wherever the language allows it; no escape hatches (`any`, unchecked casts) without a reason.

**Real-time audio & networking**
- Never block the audio thread (no allocation, locks, I/O or logging in the real-time path).
- Keep protocol handling, audio pipeline and UI in separate modules; UI never talks to the network directly.
- Stay compatible with standard Mumble servers; no private protocol extensions.

**UI**
- Keep components small. Separate state/data access from presentation.
- Accessibility: keyboard operable, screen-reader labels, respects system theme and scaling.

**Tests**
- New logic gets tests. Critical flows get an integration test along the acceptance criteria, against a local test server.
- A bug is first reproduced with a test, then fixed.
- Before "done": build, tests, type checks and linters pass. If not, say so honestly.

**Security & privacy**
- TLS always on; verify server certificates and warn clearly on changes. Protect the user's client certificate (it is their identity).
- No personal data, chat contents or audio in logs.
- Treat everything from the server (names, comments, chat HTML) as untrusted input: sanitize before rendering.
