# Profile database: migrations and compatibility

Mumcord keeps the local profile (servers, history, identities' metadata, settings, plugin data) in one SQLite file, `mumcord.sqlite3`, in the app data directory. Its schema changes through the migrations in `crates/store/src/db.rs`.

Since issue #521 the profile is **backwards compatible**: an older Mumcord can open a profile that a newer Mumcord already upgraded, as long as every newer migration only added things. This matters when people switch between a test build and a release, use two PCs on different versions, or roll back after an update.

## Two numbers in the file header

| Number | Where | Meaning |
| --- | --- | --- |
| Schema version | `PRAGMA user_version` | How many migrations were applied. |
| Minimum reader version | `PRAGMA application_id` | The oldest schema version an app must know to use this profile safely: the number of the newest **breaking** migration applied (at least 1). `0` means "not recorded" (the profile was last migrated by an app from before #521). |

Both live in the SQLite file header (bytes 60 and 68), so `store::check_schema` can read them without opening the database, and both change in the same transaction as the migration that moves them. Mumcord has no other use for `application_id`.

## What an app does on start

- Schema version ≤ the app's newest migration: it runs the missing migrations, as before.
- Schema version > the app's newest migration:
  - minimum reader version recorded and ≤ the app's newest migration: the app opens the profile as it is. It never runs migrations backwards and never touches tables, columns or settings it does not know.
  - otherwise (a breaking change, or a newer profile from before #521): the app shows the "made by a newer Mumcord" message (#512) and leaves the file byte for byte untouched.

## Writing a migration

Append a new entry to `MIGRATIONS`; never edit or reorder a released one. Wrap it in `additive(...)` or `breaking(...)`; the type makes the mark mandatory.

**Additive** – an older app ignores the change and keeps working:

- `CREATE TABLE`, `CREATE INDEX`, `CREATE UNIQUE INDEX`
- `ALTER TABLE … ADD COLUMN …` – the column must be nullable or have a `DEFAULT`, because older apps insert rows without it
- `UPDATE` that only fills a column added in the same migration
- new settings keys (they need no migration at all; see below)

**Breaking** – everything else, for example:

- renaming or dropping a table or column, rebuilding a table with a different key
- deleting or rewriting existing data
- changing what an existing value means or how it is formatted
- adding a value an older app's `CHECK` constraint or code would reject

A breaking migration raises the minimum reader version to its own number, so every older app refuses the profile with the #512 message. Prefer an additive design when one exists: add a new column or table instead of changing an old one.

The unit test `db::tests::additive_migrations_only_add` checks every additive migration statement by statement and fails if one does anything but the additive operations above. If it fails, either change the migration or mark it `breaking`.

## Writing to tables that may get new columns

Older apps write to tables whose newer columns they do not know. Writes must leave those columns alone:

- Never `INSERT OR REPLACE` / `REPLACE INTO`: it deletes the row and inserts a new one, so unknown columns fall back to their defaults. Use `INSERT … ON CONFLICT (…) DO UPDATE SET` with the columns the code knows.
- Do not rewrite a table by deleting all rows and inserting them again; update or delete single rows.
- Name the columns in every `INSERT` and `SELECT` (no `SELECT *` into fixed positions).

## Settings

The settings are one JSON record (`settings` table). Every field has a serde default, so new keys need no migration. On load, `settings_compat::decode` tolerates what a newer app wrote:

- unknown keys are ignored;
- a value the app cannot read (for example a new choice of an existing setting) is replaced by the default.

On save, `settings_compat::encode` puts both back: unknown keys are kept, and an unreadable value is restored as long as the app still has the default it showed instead. If the user changed that setting in the older app, their choice wins. Inside lists, entries have no stable identity, so a list holding something unknown is kept whole while the older app leaves it unchanged.

Removing a settings key is safe for older apps, but its value stays in the record until a newer key replaces it. Changing what an existing key means is a breaking change in spirit: give the new meaning a new key instead.

## Released history

Of the migrations v1–v29, only v26 is breaking: it rebuilt `encoder_selftest` with a new `NOT NULL` key column (`optimize`), which an app from before v26 cannot insert. So the minimum reader version of current profiles is 26. Every Mumcord that includes #521 (0.7.0 and later) can therefore open profiles from later versions as long as those only add things; versions before it cannot, because they do not know the minimum reader version.
