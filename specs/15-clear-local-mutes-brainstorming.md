# Feature – Forget local mutes on startup · Brainstorming

> Basis: Standalone feature, not part of the roadmap. Source: issue #872 ("Option to clear local mutes when Mumcord starts"), a gap found in a feature comparison with the official Mumble client (1.5 / 1.6 RC).
> Process: We work in several rounds. Each round contains questions; you answer directly in this file.
> Once all questions are answered, `specs/15-clear-local-mutes-specification.md` is created from it.
> Already settled (not asked again): Tech stack & platforms → Spec 01; settings screen structure → existing Settings UI; activity log → Spec 11. From the issue itself: the setting defaults to **off**; per-person volume, share volume and blocked users are **not** affected; client only, works against unmodified standard Mumble servers.

## How to answer

- **Single choice** (`◉`): Exactly **one** box has an `[x]`. To change your choice, just move the `x`.
- **Multiple choice** (`☑`): Tick as many boxes as you like.
- ⭐ marks my **recommendation**. It is already preselected. If you agree, you don't need to change anything.
- Below each question there is `✍️ Note:`. Add anything there, e.g. questions back, edge cases or "not sure, explain this to me".
- I explain technical terms briefly. For purely technical questions (audio, network, protocol) it's perfectly fine to follow my recommendation.

## Planned rounds

1. Functionality & scope
2. Settings UI, data & acceptance criteria

(The plan may change depending on the answers.)

---

## Round 1 – Functionality & scope

Context: a **local mute** means you silence one person only for yourself — the server and other people never see it. Mumcord remembers these mutes per server and per person, forever, even across app restarts. The official Mumble client (1.6 RC) added an option to forget them when the client starts. This feature adds the same option to Mumcord.

Good to know for the questions below:

- Local mutes are stored on your disk, grouped by server. They survive app restarts and reconnects today.
- Mumcord can stay running for days and reconnect to servers many times. "App start" and "server connect" are therefore two different moments.
- Settings import from the official Mumble client does **not** import local mutes, so there is no conflict there.

### A · When and what gets cleared

#### Q1.1 When should the mutes be forgotten? `◉`
This decides how long a local mute lives once the option is turned on.

- [x] ⭐ **Only when the app starts.** Matches the official Mumble client and the issue title. Mutes still survive reconnects while Mumcord keeps running — predictable and simple.
- [ ] Every time you connect to a server. More aggressive: a short connection drop would also wipe your mutes mid-session.
- [ ] Later: make it configurable (on start / on connect). More settings complexity than the feature deserves.

✍️ Note:

#### Q1.2 Which servers does the cleanup cover? `◉`
Mutes are stored per server. When the app starts, Mumcord may auto-connect to one server, but mutes from other servers are also on disk.

- [x] ⭐ **All servers at once.** One sweep at app start, nothing stale left anywhere. Simple rule: "when Mumcord starts, all local mutes are gone."
- [ ] Only the servers you actually connect to during this session. Keeps mutes for servers you don't visit — more surprising, more logic.

✍️ Note:

#### Q1.3 One global setting or one per server? `◉`

- [x] ⭐ **One global toggle** ("Forget local mutes when Mumcord starts"). One decision, applies everywhere. Matches how the official client does it.
- [ ] Later: a per-server override. Useful only for rare mixed setups; adds a second place to look.

✍️ Note:

### B · Feedback & extras

#### Q1.4 Should Mumcord tell you that mutes were cleared? `◉`
With the option on, people you muted yesterday will be audible again after a restart. A small notice could prevent "why is everyone unmuted?" moments.

- [x] ⭐ **Silently, like the official client.** You explicitly opted in, so no surprise. No extra noise at every start.
- [ ] A quiet entry in the activity log ("Local mutes from the last session were forgotten"). Transparent, but adds an entry on every single start.

✍️ Note:

#### Q1.5 A "Forget all local mutes now" button next to the setting? `◉`
Would let you clear mutes immediately without restarting the app. Handy, but beyond what the issue asks for.

- [x] ⭐ **Later / not in MVP.** Keep this feature to the one startup option; a manual button can come as its own small follow-up.
- [ ] Include it now. Small effort, but grows the scope and needs its own confirmation dialog and texts.

✍️ Note:

---
## Round 2 – Settings UI, data & acceptance criteria

> Note on round 1: all recommendations were accepted as preselected (chat answer "fine", 2026-11-17). So: clear only at app start, all servers at once, one global toggle (default off), silent, no manual "clear now" button in MVP.

Context for this round — how it fits into what exists:

- The Settings screen has pages (Voice, Chat & Notifications, Privacy, …) with groups of cards. Blocked users already live as a group on the **Chat & Notifications** page.
- Settings search finds controls by keywords; the new toggle gets keywords like "mute", "forget", "startup".
- Data model (technical, settled by the existing architecture — nothing to choose):

| Entity | Field | Type | Default | Rule |
|---|---|---|---|---|
| App settings | `forgetLocalMutesOnStart` | boolean | `false` | One global flag, stored like other app settings |
| Per-person setting (existing) | `local_mute` | boolean | `false` | Reset to `false` for **all** servers once at app start, before any auto-connect. `volume`, `share_volume`, blocks and friends stay untouched |

- New UI texts go through the normal translation process (all 24 locales; human pass tracked in issue #291).

Wireframe of the proposed group (shown on its settings page, above or below the blocked-users group):

```text
┌──────────────────────────────────────────────────────────────┐
│ Local mutes                                                  │
│                                                              │
│  [ ] Forget local mutes when Mumcord starts                  │
│      People you muted only for yourself become audible       │
│      again after a restart. Volumes and blocked users        │
│      are kept.                                               │
└──────────────────────────────────────────────────────────────┘
```

### A · Placement & wording

#### Q2.1 Where should the toggle live? `◉`

- [x] ⭐ **Settings → Chat & Notifications, as a "Local mutes" group next to Blocked users.** Keeps "things I do to specific people" (block, mute) in one place; the page already handles per-person behaviour.
- [ ] Settings → Voice. Muting is heard, not read — but the Voice page is about your microphone and playback, not about other people.
- [ ] Settings → Privacy. Fits "what others can do to me", but local mutes are not a privacy feature.

✍️ Note:

#### Q2.2 Label and helper text? `◉`

- [x] ⭐ **Label: "Forget local mutes when Mumcord starts" + helper: "People you muted only for yourself become audible again after a restart. Volumes and blocked users are kept."** Matches the issue wording and explains the consequence in plain language.
- [ ] Short label only ("Clear local mutes on startup"), no helper. Less text, but "local mute" is jargon some users won't know.

✍️ Note:

### B · Acceptance criteria

#### Q2.3 Are these acceptance criteria enough? `◉`
Each will be verified in the running client against a local test server:

1. Toggle exists on the settings page, is off by default, and is found via settings search ("mute", "forget").
2. Option **off**: mute a person locally, restart Mumcord → the person is still muted.
3. Option **on**: mute a person locally, restart Mumcord → the person is audible again; the mute icon is gone.
4. Option **on**: per-person volume, share volume, blocked users and friends survive the restart unchanged.
5. Option **on**: mutes on a server you do *not* connect to after the restart are cleared too.
6. Option **on**: a reconnect while the app keeps running does **not** clear mutes set during this session.
7. Turning the option on takes effect from the next app start; it does not clear anything immediately.

- [x] ⭐ **Yes, these cover it.**
- [ ] Something is missing or wrong — describe it in the note.

✍️ Note:

---

## Closing

2026-11-17: All rounds answered (round 1 and round 2 accepted as recommended, chat answer "fine"). The specification has been created: `specs/15-clear-local-mutes-specification.md`.
