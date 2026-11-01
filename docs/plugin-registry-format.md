# Plugin registry wire format (v1)

This is the reference for the files a plugin registry publishes and Mumcord reads: the
official registry (`Mumcord/plugins`, built by its publish workflow) and third-party
repositories (for example served with `mumcord-plugin serve-registry`). The client side lives in
`crates/plugins/src/registry/`. Spec: `specs/06-plugin-system-specification.md`, FR-6.49 … FR-6.57,
FR-6.70, FR-6.74 … FR-6.76, FR-6.80.

All files are UTF-8 JSON. Unknown fields are ignored, so new optional fields can be added
without a format change. A change that old clients would misread needs a new `format` number.

## Files and locations

| File | Official registry | Third-party repository |
|---|---|---|
| Index | `https://mumcord.github.io/plugins/v1/index.json` | Any URL the user adds (the address *is* the index URL) |
| Index signature | `…/v1/index.json.minisig` | Index URL + `.minisig` |
| Revocation list | `…/v1/revoked.json` + `.minisig` | – (only the official list exists) |
| Permission texts | `…/v1/permissions.json` | – |
| Package | GitHub release asset `<id>-v<ver>/<id>-<ver>.mcplugin` | Any URL listed in the index |
| Author signature | Package URL + `.minisig` | Package URL + `.minisig` |

The official base (`https://mumcord.github.io/plugins/v1/`) can be replaced by a mirror in
Settings → Plugins → Repositories → Advanced. A mirror serves the same signed files; Mumcord still
checks them with the official keys.

### Transport rules

- `https://` only. Plain `http://` is accepted only for `localhost`, `127.0.0.1` and `[::1]`, and
  only in Developer mode (also how test registries are served). Addresses must not contain a
  user name or password.
- Requests carry `User-Agent: Mumcord` and no cookies, referer, identifiers or list of installed
  plugins. At most 3 redirects, each under the same rules.
- `ETag` / `If-None-Match` is used for the index and the revocation list; servers should send an
  `ETag`. Mumcord keeps the last verified copy and works offline with it.
- Size limits: official index 8 MiB, third-party index 2 MiB, `revoked.json` 1 MiB, signature
  files 4 KiB, packages 5 MiB (and exactly the `size` the index lists).

## Signatures

Every signed file has a detached [minisign](https://jedisct1.github.io/minisign/) signature
(ed25519) next to it, created with `minisign -S -m <file>` (or `mumcord-plugin`), i.e. the
standard text `.minisig` with untrusted comment, signature, trusted comment and global signature.

- `index.json` and `revoked.json` of the official registry: signed by the official repository key.
  Mumcord has the active key (id `1CB49831E26E61AD`) and the offline backup key
  (id `7989D354F5AE80C5`) built in. Development builds also accept a test key from
  `MUMCORD_PLUGIN_REGISTRY_KEY`.
- Third-party `index.json`: signed by the repository key declared in its `repository.public_key`.
- Packages: signed by one of the plugin's `author_keys`.

Keys are given as the base64 line of a minisign public key (the second line of a `.pub` file,
e.g. `RWStYW7iMZi0HAbHe+TL+GQ4NH70MYE6hJMKZDGGvpbqucV6UpbYwVXz`). A key's **fingerprint** is its
minisign key id as 16 upper-case hex digits (`1CB49831E26E61AD`); third-party plugins are tagged
in peer announces with its first 8 hex digits.

## `index.json`

```json
{
  "format": 1,
  "sequence": 42,
  "repository": {
    "name": "Foo Repo",
    "homepage": "https://foo.example",
    "contact": "plugins@foo.example",
    "public_key": "RW…"
  },
  "key_rotation": {
    "new_key": "RW…",
    "signature": "untrusted comment: …\n…"
  },
  "plugins": [
    {
      "id": "dice",
      "name": { "en": "Dice", "de": "Würfel" },
      "summary": { "en": "Roll dice in the channel" },
      "description_md_url": "https://mumcord.github.io/plugins/v1/media/dice/README.md",
      "icon": "https://mumcord.github.io/plugins/v1/media/dice/icon-128.png",
      "screenshots": [
        { "url": "https://…/dice/shot-1.png", "alt": { "en": "A roll of two dice" } }
      ],
      "categories": ["games"],
      "authors": ["Mumcord"],
      "author_keys": ["RW…"],
      "reviewed": true,
      "updated": "2026-10-01",
      "versions": [
        {
          "version": "1.2.0",
          "api": 1,
          "url": "https://github.com/Mumcord/plugins/releases/download/dice-v1.2.0/dice-1.2.0.mcplugin",
          "size": 48213,
          "sha256": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
          "permissions": ["commands", "chat.send"]
        }
      ]
    }
  ]
}
```

| Field | Type | Required | Meaning |
|---|---|---|---|
| `format` | int | yes | Always `1` for this format. Other values are rejected. |
| `sequence` | int ≥ 0 | yes | Increases with every publish. Mumcord refuses an index with a lower `sequence` than the last one it accepted from the same source (rollback protection) and keeps its cached copy. |
| `repository` | object | third-party only | Shown in the add dialog. The official index has none. |
| `repository.name` | string | yes | Shown as "Unofficial · <name>". Not empty. |
| `repository.homepage`, `.contact` | string | no | Shown in the add dialog. |
| `repository.public_key` | key | yes | The key this file is signed with. Pinned on first use. |
| `key_rotation` | object | no | See [Key rotation](#key-rotation). |
| `plugins` | array | yes | Third-party: at most 500 entries, otherwise the whole index is rejected. |

Plugin entry:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | string | yes | Plugin id, `^[a-z][a-z0-9-]{2,31}$`; must equal the manifest id. |
| `name`, `summary` | map lang → text | yes | `en` is required; other language tags optional. |
| `description_md_url` | URL | no | Long description in Markdown (FR-6.80). |
| `icon` | URL | no | |
| `screenshots` | array of `{url, alt}` | no | `alt` is a map lang → text. |
| `categories`, `authors` | array of strings | no | |
| `author_keys` | array of keys | yes | At least one. A package must be signed by one of them. |
| `reviewed` | bool | no | Default `false`. The official registry sets it after maintainer review. |
| `updated` | `YYYY-MM-DD` | yes | Date of the latest version. |
| `versions` | array | yes | At least one; no duplicate versions. The highest semver is the latest. |

Version entry:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `version` | semver | yes | Must equal the manifest version. |
| `api` | int | yes | Plugin API major version (`1`). |
| `url` | URL | yes | Package download; author signature at `url` + `.minisig`. |
| `size` | int | yes | Exact package size in bytes, 1 … 5 MiB. |
| `sha256` | hex | yes | Lower-case hex SHA-256 of the package (64 digits). |
| `permissions` | array of ids | yes | Every permission, required or optional, the reviewed version may request. A package asking for more is refused. |

### Invalid and forbidden entries

An index is accepted or rejected as a whole only for its signature, `format`, `sequence`, the
`repository` block and the 500-plugin limit. A single bad plugin entry (missing field, bad id,
hash, version or key, duplicate id) is dropped and reported as a note, so one broken entry does
not hide the others. Third-party indexes additionally may not list official plugin ids or ids with
a reserved prefix (`mumcord-`, `core-`); such entries are dropped with a note as well.

### Install checks

For a package, Mumcord checks in this order and stops at the first failure: size, SHA-256 against
the index, author signature against `author_keys`, then the manifest against the index entry
(id, version, permissions ⊆ `permissions`).

## Key rotation

A third-party repository pins the key of its first index (trust on first use), or the "Expected
key" the user typed. When a later index is signed with a different key, Mumcord pauses the
repository and asks the user, unless the index carries a valid `key_rotation`:

- `key_rotation.new_key` equals `repository.public_key` (the key the index is signed with), and
- `key_rotation.signature` is a minisign signature **by the pinned (old) key** over the exact
  bytes of the new key's base64 line (no trailing newline).

Then Mumcord pins the new key without asking. To rotate: sign the new key line with the old key
(`printf %s "$NEW_KEY" > new.key && minisign -S -s old.key -m new.key`), put the new key in
`repository.public_key`, put the line and the content of `new.key.minisig` into `key_rotation`, and
sign the index with the new key. Keep `key_rotation` in the index until all users have updated.

## `revoked.json`

```json
{
  "format": 1,
  "sequence": 3,
  "revoked": [
    {
      "id": "echolings",
      "versions": ["1.0.0"],
      "severity": "disable",
      "reason": { "en": "Crashes voice", "de": "Bringt Sprache zum Absturz", "es": "Bloquea la voz" },
      "since": "2026-10-20"
    },
    {
      "id": "foo",
      "sha256": ["9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"],
      "author_key": "RW…",
      "severity": "warn",
      "reason": { "en": "Leaked author key" },
      "since": "2026-10-21"
    }
  ]
}
```

| Field | Type | Required | Meaning |
|---|---|---|---|
| `format` | int | yes | `1`. |
| `sequence` | int ≥ 0 | yes | Increases with every publish; a lower one than the cached copy is refused. |
| `revoked[].id` | string | yes | Plugin id. Applies to every source (official, third-party, developer). |
| `revoked[].versions` | array of semver | no | Revoked versions. |
| `revoked[].sha256` | array of hex | no | Revoked packages by hash. Without `versions` and `sha256`, every version is revoked. |
| `revoked[].author_key` | key | no | If set, only packages signed by this author key match. |
| `revoked[].severity` | `disable` / `warn` | yes | `disable` stops and turns the plugin off ("Turned off by Mumcord: <reason>"); `warn` shows the reason on the plugin row. Data is never removed. |
| `revoked[].reason` | map lang → text | yes | `en` required; `de` and `es` expected. |
| `revoked[].since` | `YYYY-MM-DD` | yes | Publication date. |

Mumcord fetches `revoked.json` once a day, even with plugin updates off, and always applies the
copy bundled with the release (`crates/plugins/assets/revoked.json`) in addition to the fetched
one. If several entries match, `disable` wins over `warn`.

## `permissions.json`

The official registry also publishes `v1/permissions.json` with the shared permission texts for
the website (FR-6.80). It is generated from the client's permission catalogue and not signed.

```json
{
  "format": 1,
  "permissions": {
    "chat.send": { "risk": "yellow", "official_only": false, "text": { "en": "…" } }
  }
}
```

`risk` is `green`, `yellow` or `red`; `text` maps language tags to the text shown in the install
prompt (English only for now).

## Registry source

The official registry is generated from files in `Mumcord/plugins` (FR-6.54) by
`mumcord-plugin site-data`; the publish workflow then signs `index.json` and `revoked.json` with
`mumcord-plugin sign`. Authors add a version with `mumcord-plugin publish-pr`, which writes the
files below and opens the pull request. All files are TOML; unknown fields are an error, so typos
are caught in review.

```
plugins/<id>/plugin.toml          one per plugin; the folder name is the id
plugins/<id>/versions/<ver>.toml  one per published version; the file name is the version
plugins/<id>/<media files>        README, icon, screenshots referenced from plugin.toml
revoked.toml                      kill switch entries
```

### `plugins/<id>/plugin.toml`

```toml
name = { en = "Dice", de = "Würfel" }
summary = { en = "Roll dice in the channel" }
categories = ["games"]
authors = ["Mumcord"]
author_keys = ["RW…"]
reviewed = true
description = "README.md"
icon = "icon-128.png"

[[screenshots]]
file = "shot-1.png"
alt = { en = "A roll of two dice" }
```

| Field | Required | Meaning |
|---|---|---|
| `name`, `summary` | yes | Language tag → text; `en` required. Become `name` / `summary` in the index. |
| `categories`, `authors` | no | Copied into the index. |
| `author_keys` | yes | Minisign public keys (base64 line) allowed to sign packages. A new key needs maintainer review. |
| `reviewed` | no | Set by a maintainer after review; default `false`. |
| `description` | no | Markdown file in the plugin folder, published as `v1/media/<id>/<file>` (`description_md_url`). |
| `icon` | no | Image in the plugin folder, published under `v1/media/<id>/`. |
| `screenshots` | no | `{file, alt}`; `alt` maps language tags to text. Published under `v1/media/<id>/`. |

File paths are relative to the plugin folder and may not contain `..`.

### `plugins/<id>/versions/<ver>.toml`

```toml
version = "1.2.0"
api = 1
size = 48213
sha256 = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
permissions = ["commands", "chat.send"]
published = "2026-10-01"
signature = """
untrusted comment: signature from minisign secret key
RW…
trusted comment: …
…
"""

[source]
repository = "https://github.com/Mumcord/plugins"
commit = "4f1c…"
path = "dice"
```

| Field | Meaning |
|---|---|
| `version`, `api`, `size`, `sha256`, `permissions` | Become the version entry of the index. `permissions` is every permission the manifest asks for. |
| `published` | `YYYY-MM-DD`; the newest version's date becomes `updated`. |
| `signature` | The author's detached minisign signature over the package; published as the release asset `<id>-<ver>.mcplugin.minisig`. |
| `source` | Git repository, commit and folder the package is rebuilt from with `mumcord-plugin build --reproducible`; CI compares the hash. |

The package itself is not in the repository: the publish workflow rebuilds it from `source` and
attaches it to the release `<id>-v<ver>`, so the index URL is
`https://github.com/Mumcord/plugins/releases/download/<id>-v<ver>/<id>-<ver>.mcplugin`.

### `revoked.toml`

Same fields as the entries of [`revoked.json`](#revokedjson); `sequence` and `format` are added on
publish.

```toml
[[revoked]]
id = "echolings"
versions = ["1.0.0"]
severity = "disable"
reason = { en = "Crashes voice", de = "Bringt Sprache zum Absturz", es = "Bloquea la voz" }
since = "2026-10-20"
```

## Local test registry

`mumcord-plugin serve-registry --packages <dir> --key <test.key>` serves the packages in `<dir>`
as an official registry on `http://127.0.0.1:<port>/v1/`, signed with a test key (create one
with `mumcord-plugin keygen --unencrypted`). Point a development build at it with the mirror
setting and `MUMCORD_PLUGIN_REGISTRY_KEY=<public key line>`. With `--repository <name>` it serves
a third-party repository instead; its address is `http://127.0.0.1:<port>/v1/index.json`
(Developer mode allows plain `http` for localhost).

Scenario switches (`--scenario`, repeatable, or `POST /_ctl/scenario?set=a,b` while running; an
empty `set=` clears them) produce what the client must refuse:

| Scenario | Effect |
|---|---|
| `tampered-hash:<id>` | The index lists a wrong SHA-256 for the plugin → "Download didn't match the official list". |
| `lower-sequence` | The index sequence is one below the current one → rollback refused. |
| `wrong-key` | The index is signed with an unknown key → "Signature check failed". |
| `extra-permission:<id>` | The index allows one permission less than the manifest asks for → install refused. |
| `revoke:<id>[@<ver>][:warn]` | Adds a `revoked.json` entry (severity `disable`, or `warn`). |
| `key-change[:rotate]` | The index is signed with a second key; `rotate` adds a valid `key_rotation`. |

`POST /_ctl/reload` rescans the package folder and raises the sequence, which publishes new
versions. The sequence starts at the current Unix time unless `--sequence` is given.
