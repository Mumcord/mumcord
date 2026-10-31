# Epic 3 – Comfort & polish · Brainstorming

> Basis: overview issue #63 (Epic 3: Comfort and polish) and all issues in GitHub milestone 0.5.0.
> Process: Epic 3 was not planned in brainstorming rounds. It was planned directly in GitHub issues: each feature got its own issue with a description, mockups (wiki `images/mockups/epic3/…`) and decisions recorded as comments. Several features were built straight from their issues.
> On 2026-10-30 `specs/03-comfort-polish-specification.md` was documented from those issues, the merged and open PRs and the CHANGELOG.
> Already settled (not asked again):
> - Tech stack, platforms, architecture, accessibility, privacy rules → Spec 01.
> - Multi-server read-along, themes, P2P layer, watch party and screen sharing → Spec 02.
> - Identity manager and server dialog details → `specs/03-identity-manager-specification.md`, `specs/03-server-dialog-specification.md` (PR #197).
> - Decisions already made in issue comments → spec section 12.

## How to answer

- **Single choice** (`◉`): Exactly **one** box has an `[x]`. To change your choice, just move the `x`.
- **Multiple choice** (`☑`): Tick as many boxes as you like.
- ⭐ marks my **recommendation**. It is already preselected. If you agree, you don't need to change anything.
- Below each question there is `✍️ Note:`. Add anything there, e.g. questions back, edge cases or "not sure, explain this to me".

---

## Open points

Contradictions between issues, and gaps found while documenting the spec. Each answer updates the spec and the affected issue.

### A · Scope and rules

#### Q1.1 How does the mumpeer relay (#62) fit the "client only" rule? `◉`
#62 needs an optional relay server (mumpeer) and a bot module (mumbot). Spec 02 (FR-2.35) says "no STUN, TURN or other external server", and CLAUDE.md says "client only, no companion services". #63 already notes that CLAUDE.md needs an addendum.

- [x] ⭐ **Add a CLAUDE.md addendum and amend FR-2.35: optional companion services in their own repos are allowed; Mumcord must work fully without them.** Then write the #62 spec before building anything. Keeps the rule's intent (nothing required on the server).
- [ ] Move #62 (and #190) out of Epic 3 to a later epic. Epic 3 stays strictly client only.
- [ ] Drop the relay idea.

✍️ Note:

#### Q1.2 Is the Settings redesign (#161) built in 0.5.0? `◉`
#161 says "design only in this issue; the build gets its own spec", but it is in milestone 0.5.0, and other Epic 3 features (identities, notification centre, blocks, presets) need a home in Settings.

- [x] ⭐ **Yes: write a short #161 build spec and build it in 0.5.0, after the features that add new settings.** New settings land in the new layout once, not twice.
- [ ] No: keep today's Settings layout for 0.5.0; new settings go into the existing sections; build #161 later.

✍️ Note:

#### Q1.3 Which steps does the wizard rerun (#149) show? `◉`
#149 says 4 steps (microphone, privacy, update check, talk mode). #159 later defines rerun as Welcome (optional) → Voice → Setup style → [Custom: detailed steps] → Summary.

- [x] ⭐ **Follow #159: rerun = the new wizard without the identity step.** One wizard, one flow; #149 only adds the entry point, prefill, Cancel and "save only changes".
- [ ] Keep the 4-step rerun from #149 as a separate, shorter flow.

✍️ Note:

### B · Privacy and notifications

#### Q1.4 Is a block (#140) per server or global? `◉`
The identity spec (D11) already says blocks are kept per own identity. Open is whether blocking someone on one server also blocks them on other servers.

- [x] ⭐ **Global for that identity: one block covers every server** (keyed by certificate hash). Someone you don't want to hear from is unwanted everywhere; the Blocked users list still shows where you blocked them.
- [ ] Per server: block only on the server where you clicked Block.

✍️ Note:

#### Q1.5 What does Do Not Disturb (#69) let through, and do others see it? `☑`

- [x] ⭐ Badges and the notification centre keep counting (already decided).
- [ ] Friends still notify during DND.
- [ ] `@all` / `@channel` still notify during DND.
- [ ] Show a DND status to other Mumcord users (via plugin data).
- [ ] Turn DND on automatically while screen sharing.
- [ ] Turn DND on automatically while a game is running.

Recommendation: start with no exceptions and no automatic modes; following the system DND already covers most of it.

✍️ Note:

#### Q1.6 Who may use `@all` and `@channel` (#70)? `◉`

- [x] ⭐ **Everyone may type them; each user can turn off notifications for `@all` / `@channel` (per server).** Simple, no permission guessing; spam is handled by the receiver's opt-out and by blocking.
- [ ] Only users with a certain permission in the channel (checked client-side from the ACL). Less spam, but not enforceable: official clients and modified clients can still send the text.
- [ ] Only `@channel`; no `@all`.

✍️ Note:

#### Q1.18 Do connection events go into the notification centre (#143)? `◉`

- [ ] ⭐ **No.** The notification centre is for people (mentions, replies, requests, invites) and the update entry; connection problems stay in the banner and connection info.
- [x] Yes, e.g. "server X unreachable" after the banner has hidden itself (#88).

✍️ Note:

#### Q1.15 Is the private message rule (#141) per server or per identity? `◉`
#141 confirms a per-server override. The identity spec (D11) says "PMs from friends only" is per own identity. Both can be true, but the spec must say how they combine.

- [x] ⭐ **One global rule plus a per-server override; accepted conversations and blocks are per own identity.** The rule is about the place (big public server vs. friends' server); the list of accepted people is about who you are.
- [ ] Everything per own identity, no per-server override.

✍️ Note:

### C · Audio

#### Q1.9 Does low-latency mode (#156) also send voice over the direct P2P path in 0.5.0? `◉`
PR #198 builds the mode without it.

- [x] ⭐ **No, later.** It needs work on both sides and ideally the relay (#62); the server path already reaches every client.
- [ ] Yes, add it to Epic 3 as a follow-up issue.

✍️ Note:

#### Q1.10 What is a "server profile" in the per-channel audio precedence (#171)? `◉`
#171 says "channel profile > server profile (if any) > global". No per-server audio setting exists anywhere.

- [x] ⭐ **Drop it: channel profile > global setting.** Nothing else asks for a server-wide audio profile.
- [ ] Add per-server audio profiles too (bitrate, music quality, low latency).

✍️ Note:

#### Q1.13 Which noise suppression choices does the wizard show (#159 vs. #176)? `◉`
#159's Voice step says "Off / Standard / Strong"; #176 defines "Standard (light)" (nnnoiseless, default), "High quality (Beta, more CPU)" (RNNoise 2024) and Off.

- [x] ⭐ **Use #176's names everywhere: Standard / High quality (Beta) / Off.** One set of options in wizard and Settings; there is no separate "Strong" level.
- [ ] Show only Standard / Off in the wizard; High quality only in Settings (Beta).

✍️ Note:

### D · Connections and settings model

#### Q1.14 Which model does the peer-to-peer / relay setting use? `◉`
Three issues describe it differently: #62 "Use relay: Off / Only when direct fails / Always"; #159 "Direct + relay / Direct only / Relay only / Off" with a priority switch; #161 "Direct connections: Ask me / Always / Never" plus a relay row.

- [x] ⭐ **#159's model is the one setting key: Direct + relay (direct first) / Direct + relay (always relay, hides my IP) / Direct only / Relay only / Off.** Wizard, Settings and the per-server override all use it; "Ask me" stays as today's per-connection consent (FR-2.36). Until #62 exists, only "Direct only" and "Off" are offered.
- [ ] Keep two separate settings: "Allow direct connections" and "Use relay" (#62).

✍️ Note:

#### Q1.7 How are hotkeys, whisper targets and the tray set up (#64, #67, #65)? `☑`

- [x] ⭐ No global hotkeys set by default (users pick them), global for all servers.
- [x] ⭐ Whisper targets are created in Settings and also from the person and channel menus ("Whisper to…"), each with an optional hotkey.
- [x] ⭐ GNOME without a tray: no tray menu; closing the window follows the existing close setting.
- [ ] Per-server hotkeys.
- [ ] A default hotkey for mute and deafen.

✍️ Note:

#### Q1.8 Where does the public server list come from (#68)? `◉`

- [x] ⭐ **The public list the official Mumble client uses, loaded only when the browser opens; check its terms of use first, and drop the feature if they forbid third-party clients.** Same list users know; nothing loads in the background.
- [ ] A Mumcord-curated list hosted on the website.

✍️ Note: Checked 2026-10-30: no terms of use; the list answers 200 to an honest `User-Agent: Mumcord/<version>` when `Accept-Encoding: gzip` is sent (501 without it). Implemented in #68.

#### Q1.11 Who controls the watch party, and what is picture-in-picture (#163)? `◉`

- [x] ⭐ **Everyone may control playback as today; "Request control" is dropped. Picture-in-picture = the pop-out window (#164) with always on top.** Matches today's behaviour and avoids two PiP solutions.
- [ ] Only the controller plays, pauses and seeks; others use "Request control".

✍️ Note:

### E · Updates and release

#### Q1.12 Updater details for 0.5.0 (#168)? `☑`

- [x] ⭐ Stable channel only in 0.5.0; Beta channel later.
- [x] ⭐ AppImage via the Tauri updater plugin (same path as Windows and macOS).
- [x] ⭐ The portable Windows zip (#181) gets "Notify only" (no self-replace); #181 assumed an exe replaced in place, but the build is a zip.
- [x] ⭐ The minisign private key is created by the maintainer and stored only as a CI secret; rotation is documented in `docs/release-signing.md`.
- [ ] Preset mapping: Maximum privacy = Off (instead of Notify only).

✍️ Note:

### F · Gaps in the already written work

#### Q1.16 The server dialog spec still says channel passwords stay numbered until #146 (D9). `◉`
#146 is merged (PR #189), so the Passwords tab of PR #197 must show the labelled passwords.

- [ ] ⭐ **Update the server dialog spec and PR #197 to use the #146 labels before merging.**
- [x] Merge PR #197 as is and fix the Passwords tab in a follow-up.

✍️ Note:

#### Q1.17 What does the Epic 3 verification (#195) cover? `◉`
#195 lists only the identity manager and the server dialog.

- [x] ⭐ **Extend #195 to every Epic 3 acceptance criterion (AC-3.50 … AC-3.89 plus the sub-specs) before the 0.5.0 release.**
- [ ] Keep #195 for #148 / #151; each other feature verifies itself in its own PR.

✍️ Note:

---

## Closing

Date: 2026-10-30. The answers were given in chat. The specification (`specs/03-comfort-polish-specification.md`) was updated from them.
