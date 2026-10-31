# Epic 4 – mumpeer relay · Brainstorming

> Basis: issue #62 (mumpeer relay) and issue #190 (server info panel shows an announced relay); mumbot issue Mumcord/mumbot#8 (peer module).
> Process: the decisions were made in a chat brainstorm with the product owner on 2026-10-31 and are recorded below. `specs/04-mumpeer-relay-specification.md` was written from them. The questions under "Open points" came up while writing the spec; the recommended answer is preselected and already used in the spec. Change a choice here and the spec is updated.
> Already settled (not asked again):
> - Tech stack, platforms, architecture, accessibility, privacy rules → Spec 01.
> - P2P layer (`mumcord-p2p`, QUIC with certificate pinning, plugin-data signalling, consent) → Spec 02 FR-2.35, FR-2.36.
> - Optional companion services in their own repos are allowed; Mumcord must work fully without them → Spec 03 Q1.1.
> - One P2P / relay setting key with the #159 model → Spec 03 Q1.14.

## How to answer

- **Single choice** (`◉`): Exactly **one** box has an `[x]`. To change your choice, just move the `x`.
- **Multiple choice** (`☑`): Tick as many boxes as you like.
- ⭐ marks my **recommendation**. It is already preselected. If you agree, you don't need to change anything.
- Below each question there is `✍️ Note:`. Add anything there, e.g. questions back, edge cases or "not sure, explain this to me".

## Decisions from the chat brainstorm (2026-10-31)

1. **Architecture: three parts.** mumpeer, a new Rust relay server in `Mumcord/mumpeer`, run by server communities; a mumbot module "peer" in `Mumcord/mumbot`; the Mumcord client. The Mumble server stays unmodified. Mumcord works fully without bot and relay (Q1.1 of spec 03).
2. **Transport.** The relay forwards the inner, end-to-end encrypted, certificate-pinned QUIC packets of `mumcord-p2p` unchanged, like TURN. It never terminates the inner session. It only sees encrypted bytes, volume, and who talks to whom.
3. **TCP fallback.** TLS over TCP port 443 for networks that block UDP is in scope now.
4. **Access to the relay.** Mutual TLS with the user's Mumble client certificate plus a short-lived token bound to the pair of peers. The relay only accepts certificate hashes on the allowlist that the bot registered. It must never act as an open proxy.
5. **Discovery in two stages.**
   1. On join, after the Mumcord hello, the bot announces to that client only, via plugin data: the relay address and the relay certificate fingerprint. It re-announces to all clients when the bot or the relay restarts or the address changes.
   2. When a P2P session is needed and direct fails, or "hide my IP" is on, the client asks the bot for a token, valid for that pair only, for a few minutes.
   - If the bot is gone, the relay counts as unavailable after a short timeout, with a clear reason shown.
6. **No Mumcord-hosted public relay.** Communities only.
7. **No manual relay address without the bot, ever.**
8. **No voice over the relay.** It is for P2P features such as screen sharing only.
9. **Client setting model per Q1.14.** Direct + relay (direct first) / Direct + relay (always relay, hides my IP) / Direct only / Relay only / Off; the per-server override uses the same key. Connection info shows "direct" or "via relay" and whether the IP is hidden. The server info panel shows that a relay is announced (#190). The packet inspector shows the announcements.
10. **Relay operations.** A single Rust binary plus a Docker image; one UDP port plus TCP 443; a small config file; limits for bandwidth per session and in total, max sessions, rate limits; minimal logging with no IPs or certificate hashes beyond what operations need (the spec says which); the bot↔relay allowlist API is authenticated with a secret from local config, never committed.

---

## Open points

### A · Client behaviour

#### Q1.1 What does "Direct + relay (always relay, hides my IP)" do where it differs from "Relay only"? `◉`
Both use only the relay when the server has one. The difference matters on servers **without** a relay (most servers today).

- [x] ⭐ **On servers without a relay it falls back to direct and shows "IP visible"; "Relay only" never goes direct anywhere.** Keeps screen sharing working on plain servers and makes the two options really different.
- [ ] It never goes direct either; the two options are the same and "Relay only" is dropped from the list.
- [ ] It asks each time before falling back to direct on a server without a relay.

✍️ Note:

#### Q1.2 Does using the relay need a consent question of its own? `◉`
Today Mumcord asks once before the first direct connection, because the other person sees your IP. With the relay, the other person doesn't see it, but the relay's operator (the server community) does, just like the Mumble server itself.

- [x] ⭐ **No extra question. Choosing a relay mode is the consent; the existing direct-connection question gets one sentence about the relay, and answering "No" there still allows the relay.** The relay operator is the same community that runs the Mumble server, which already sees your IP.
- [ ] Ask once per server before the first relay connection ("This server's relay will see your IP address").
- [ ] Ask once globally before the first relay connection.

✍️ Note:

### B · Trust

#### Q1.3 Which announcement does the client trust? `◉`
Anyone on a Mumble server can send plugin data. A fake "bot" could announce its own relay. It can't read or change the screen share (the inner encryption is pinned to the users' certificates), but it would see the IP addresses of users who use it.

- [x] ⭐ **Only from registered users on that server, and pinned on first use: if the bot or the relay's certificate later changes, Mumcord asks before using it ("The relay of this server changed"). Two announcers at once → none is used.** Works on any standard server, like Mumble's own certificate warning.
- [ ] Only from registered users, no pinning (simpler, but a later rogue announcer wins silently).
- [ ] Only if the server's welcome text contains the relay fingerprint (needs admins to edit the welcome text; harder setup).

✍️ Note:

#### Q1.4 When does the relay learn a user's certificate hash? `◉`
Issue #62 says the bot registers every user it sees. The alternative registers only users who actually need the relay.

- [x] ⭐ **Only when a pair token is created (on demand).** The relay learns only the two users of a session, nothing about everyone else on the server. Same protection against open-proxy use.
- [ ] Every Mumcord user the bot sees is registered at join, as written in #62.

✍️ Note:

### C · mumbot

#### Q1.5 How do mumbot's own rules change for the peer module? `☑`
mumbot's CLAUDE.md still says "no companion services", and the bot currently accepts any Mumble server certificate. A bot that hands out relay tokens should know it talks to the real server.

- [x] ⭐ Amend mumbot's CLAUDE.md like Mumcord's (optional companion services in their own repos allowed; the bot must work fully without them).
- [x] ⭐ The peer module requires a pinned Mumble server certificate (`server_cert_sha256` in the config) and refuses to start without it.
- [ ] Pin the server certificate for all mumbot bots now (wider change, its own mumbot spec).

✍️ Note:

#### Q1.6 How is the peer bot started before mumbot's modular bot (mumbot#5) exists? `◉`

- [x] ⭐ **As its own subcommand `mumbot peer --config bot.toml`, like the webhook bot; it becomes the module `peer` with the same config section once #5 is built.** Doesn't wait for #5.
- [ ] Build #5 first and ship peer only as a module.

✍️ Note:

---
