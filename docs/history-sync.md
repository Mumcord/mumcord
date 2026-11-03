# History sync: locked channels and the password proof

Mumcord clients fill each other's chat history through plugin data with the data id
`mumcord.history` (FR-1.60). The packet format and the normal flow (`areq`, `avail`, `hreq`,
`hashes`, `get`, `part`, `deny`) are described in `crates/engine/src/history_sync/wire.rs` and
in [engine-api.md](engine-api.md) under "History sync". This page covers the rules for locked
channels (issue #382) and is the contract for every program that answers history requests,
Mumcord and mumbot alike.

## Why

On a standard Mumble server, _Listen_ is a separate permission from _Enter_ and everyone has it
by default. A channel locked with a password (access token) or an ACL that denies _Enter_
still lets anyone listen to it. History sync used to answer everyone who received a channel
live, listeners included, so anyone could collect a week of a locked channel's chat.

## Locked channels

A channel is **locked** when it or any of its parent channels has `is_enter_restricted` set
(`ChannelState`, Mumble 1.4+). The server sets the flag only from a channel's own ACL, so an
inherited lock is found through the parent. A locked root locks every channel. A channel the
client does not know counts as locked.

Rules for a locked channel:

- **Answerers** answer only requesters who are **physically in the channel** right now
  (`UserState.channel_id`; the server checks _Enter_ for every self-move). Listening is not
  enough. This is checked for `areq`, for `hreq` and again before sending messages for `get`.
  An open answer ends as soon as the requester leaves the channel. Everything else stays as it
  is: messages only from before the requester started receiving the channel live, at most
  7 days and 500 messages, the per-person request budget and the limit of open answers.
- **Answerers holding access tokens** also ask for the password proof (below) before they send
  the hash list. A client cannot tell a password channel from one locked by groups, so an
  answerer with at least one token asks for the proof for every locked channel.
- **Answerers without any token** (they got in through groups, registration or an admin)
  answer locked channels on physical presence alone.
- **Requesters** ask for a locked channel's history only while they are inside it. Following
  a locked channel brings no history; "Follow all" skips channels the user may not enter.

Open channels behave as before: whoever is in a channel or listens to it is answered.

## The password proof

### Packets

Both use the normal packet layout: a JSON header, `\n`, then a binary body. Version stays
`v: 1`; programs that do not know the kinds ignore them as malformed.

| `t`     | Header fields                                   | Body                                                  | Direction            |
| ------- | ----------------------------------------------- | ----------------------------------------------------- | -------------------- |
| `chal`  | `r` (the `hreq`'s request id), `c` (channel id) | exactly 16 bytes: the nonce                           | answerer → requester |
| `proof` | `r` (the same request id)                       | 0 to 16 proofs of exactly 32 bytes each, concatenated | requester → answerer |

Example: `{"v":1,"t":"chal","r":7,"c":12}\n<16 random bytes>`.

### Computing a proof

```
K     = HMAC-SHA256(key = "mumcord.history.token.v1" (ASCII), message = token (UTF-8))
proof = HMAC-SHA256(key = K, message =
          nonce (16 bytes)
        ‖ channel id (u32, big endian)
        ‖ len(requester cert hash) (u8) ‖ requester cert hash (ASCII)
        ‖ len(answerer cert hash) (u8)  ‖ answerer cert hash (ASCII))
```

- The certificate hashes are the ones the server reports in `UserState.hash` (40 lowercase hex
  characters for SHA-1).
- The requester sends one proof per access token it holds, at most 16, in any order.
- The token itself, or a plain hash of it, is never sent.

### Answerer

On `hreq` for a locked channel, while holding at least one access token:

1. Check the requester as for any locked channel (sharing on, physically inside, budget,
   fewer than 3 open answers and challenges). Otherwise `deny`.
2. Requester or own certificate hash missing: `deny`.
3. Make a fresh nonce of 16 random bytes from a cryptographic random source. Remember it
   together with the requester's session, the request id `r`, the channel, the requested range
   and the time.
4. Send a `PermissionQuery` for that channel for yourself, then send `chal`.

On `proof` from the requester for `r`:

1. Find the challenge of exactly this requester session and request id. None: ignore the
   packet. A challenge is removed by the first `proof` that names it, valid or not, so a nonce
   is never accepted twice.
2. The challenge is older than 10 s: `deny`. Challenges nobody answers get a `deny` after
   10 s too.
3. The server's answer to the own `PermissionQuery` for that channel must have arrived after
   the challenge was made, with _Enter_ (`0x04`) set. Otherwise (for example the password
   was changed on the server and the own token is outdated): `deny`.
4. The requester must still be inside the channel: otherwise `deny`.
5. Recompute the proof for every own token and compare each received proof in constant time.
   No match: `deny`.
6. Match: send the `hashes` as for an open channel and keep the answer for `get`. Before
   sending messages for that `get`, check again that the requester is inside and that the last
   known own _Enter_ permission there is still set; otherwise `deny`.

### Requester

On `chal` for an own open `hreq` `r` to that answerer, for the same channel, while still
waiting for its hash list:

- Send `proof` only if the answerer is **physically in that channel** (so nobody outside it can
  collect proofs to guess a password offline), the own certificate hash is known and the
  client holds at least one access token.
- Otherwise give up on that answerer at once and continue with the others.

### Deny and silence at a glance

| Situation                                                      | Answer  |
| -------------------------------------------------------------- | ------- |
| `areq` for a locked channel from someone not inside it         | silence |
| `hreq` for a locked channel from someone not inside it         | `deny`  |
| No certificate hash on either side (proof needed)              | `deny`  |
| Proof too late (> 10 s), wrong, or own _Enter_ not confirmed   | `deny`  |
| Unanswered challenge after 10 s                                | `deny`  |
| `proof` without a matching challenge (replay, other requester) | silence |
| `get` after the requester left or the own _Enter_ was lost     | `deny`  |

## Compatibility

- Older Mumcord requesters never answer `chal`: they get no history from answerers with
  tokens (fail closed). Answerers without tokens still answer them while they are inside.
- Older Mumcord answerers do not know these rules and still answer listeners. Only updating
  them closes the leak on their side.
- The server forwards plugin data and can read it. That is accepted: the server admin sets the
  password anyway. Encrypting history to the requester's certificate is a possible later
  improvement and out of scope here.

## What mumbot needs

mumbot answers history requests for the channels it records, so it follows the same rules:

- Use the locked definition above, with parent channels.
- Track each person's physical channel separately from the channels they listen to, and answer
  locked channels only to people inside them, checked at `areq`, `hreq` and `get`; end open
  answers when the requester leaves.
- Take the channel tokens from its configuration (the tokens it logs in with). With at least
  one token, demand the password proof for locked channels exactly as described for the
  answerer, including the self `PermissionQuery` per challenge, the 10 s single-use nonce and the
  constant-time comparison; without tokens, answer on presence only.
- A bot that only listens to a locked channel is not inside it, so Mumcord requesters will not
  send it proofs: with tokens it can share a password channel's history only while it is
  inside that channel.
