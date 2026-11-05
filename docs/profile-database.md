# Profile database: migrations and compatibility

Mumcord keeps the local profile (servers, history, identities' metadata, settings, plugin data) in one SQLite file, `mumcord.sqlite3`, in the app data directory. Its schema changes through the migrations in `crates/store/src/db.rs`.

Since issue #521 the profile is **backwards compatible**: an older Mumcord can open a profile that a newer Mumcord already upgraded, as long as every newer migration only added things. This matters when people switch between a test build and a release, use two PCs on different versions, or roll back after an update.

## Two numbers in the file header

| Number                 | Where                   | Meaning                                                                                                                                                                                                                           |
| ---------------------- | ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Schema version         | `PRAGMA user_version`   | How many migrations were applied.                                                                                                                                                                                                 |
| Minimum reader version | `PRAGMA application_id` | The oldest schema version an app must know to use this profile safely: the number of the newest **breaking** migration applied (at least 1). `0` means "not recorded" (the profile was last migrated by an app from before #521). |

Both live in the SQLite file header (bytes 60 and 68), so `store::check_schema` can read them without opening the database, and both change in the same transaction as the migration that moves them. Mumcord has no other use for `application_id`.

## What an app does on start

- Schema version ≤ the app's newest migration: it runs the missing migrations, as before.
- Schema version > the app's newest migration:
  - minimum reader version recorded and ≤ the app's newest migration: the app opens the profile as it is. It never runs migrations backwards and never touches tables, columns or settings it does not know.
  - otherwise (a breaking change, or a newer profile from before #521): the app shows the "made by a newer Mumcord" message (#512) and leaves the file byte for byte untouched. If the profile can be rolled back (see below), the message also offers **Roll back the profile to this version** next to Download and Quit.

## Rolling back

Every migration comes with a **down step**: SQL that restores the schema from before it. Since v30 the app stores the down step of each migration it applies in the profile itself, in the table `migration_down (version, breaking, sql)`; v30 fills in the steps of v1–v30 from the code. So an older app can undo migrations it does not know, using only what the newer app stored.

A rollback only runs when it is needed and the user asks for it:

- It is offered only when the profile is unreadable for this app (minimum reader version > the app's newest migration).
- It goes back to the version just before the **oldest breaking migration** the app does not know, not further. Additive migrations below that one stay: the app can work with them. Additive migrations above it are undone too, because the breaking one below them has to go.
- It is offered only if every migration it has to undo has a stored, reversible down step. Otherwise the message offers only Download and Quit.

When the user confirms, the app

1. checkpoints the WAL and copies the database file to `mumcord.sqlite3.backup-<YYYYMMDD-HHMMSS>` beside it (local time; an existing file is never overwritten),
2. runs the stored down steps from the newest migration on, in **one transaction**, with foreign keys off so a rebuilt table cannot cascade into older tables,
3. deletes the undone steps from `migration_down` and sets `user_version` to the target and `application_id` to its own minimum reader version,
4. and continues starting.

If a down step fails, the transaction is rolled back and the profile stays byte for byte as it was; the backup copy remains.

`store::rollback_plan` decides whether a rollback is possible without changing any file (it opens the database read-only, as an immutable file when no WAL exists); `store::roll_back` performs it.

## Writing a migration

Append a new entry to `MIGRATIONS`; never edit or reorder a released one. Wrap it in `additive(...)` or `breaking(...)`; the type makes the mark mandatory.

Every migration also gets a down step, `Down::Sql("…")` or `Down::Irreversible`:

- The down step may only remove what its migration added (drop the tables, indexes and columns it created) or rebuild what it changed, so only data written since the migration is lost. A breaking migration's step restores the old form and keeps whatever data the old form can hold.
- Mark the migration `Down::Irreversible` when undoing it would lose data from before it, or when it created certificate or identity data (identities, trusted certificates, certificate pins) and is additive. A profile that needs an irreversible migration undone is never rolled back. Today v1, v14, v20 and v21 are irreversible; they are all older than the stored down steps, so no rollback ever reaches them.
- `ALTER TABLE … DROP COLUMN` cannot drop a column with a foreign key, `UNIQUE` or an index; rebuild the table or mark the migration irreversible then.

The unit test `db::tests::every_down_step_round_trips_on_a_seeded_profile` runs every reversible migration up, down and up again on a profile with data and checks that schema and rows match each time.

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

Of the migrations v1–v30, only v26 is breaking: it rebuilt `encoder_selftest` with a new `NOT NULL` key column (`optimize`), which an app from before v26 cannot insert. So the minimum reader version of current profiles is 26. Every Mumcord that includes #521 (0.7.0 and later) can therefore open profiles from later versions as long as those only add things; versions before it cannot, because they do not know the minimum reader version. Rolling back works between versions that both include v30 (the stored down steps).
