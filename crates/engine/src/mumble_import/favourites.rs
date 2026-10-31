//! Reads the server favourites from the official client's `mumble.sqlite`.

use std::path::Path;

use rusqlite::{Connection, OpenFlags};

/// Mumble's default server port.
const DEFAULT_PORT: u16 = 64738;

/// A favourite from Mumble's `servers` table. `Debug` leaves out the password.
#[derive(Clone, PartialEq, Eq)]
pub struct Favourite {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
}

impl std::fmt::Debug for Favourite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Favourite")
            .field("name", &self.name)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("has_password", &self.password.is_some())
            .finish_non_exhaustive()
    }
}

/// Reads all favourites, in Mumble's order (by name). The file is opened read-only and as
/// immutable, so Mumble's own database is never changed or locked, even while it runs.
pub fn read(path: &Path) -> rusqlite::Result<Vec<Favourite>> {
    let uri = format!(
        "file:{}?mode=ro&immutable=1",
        percent_encode(&path.to_string_lossy())
    );
    let conn = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )?;
    let mut stmt = conn.prepare(
        "SELECT name, hostname, port, username, password FROM servers ORDER BY name, id",
    )?;
    let rows = stmt.query_map([], |row| {
        let text = |i: usize| -> rusqlite::Result<String> {
            Ok(row
                .get::<_, Option<String>>(i)?
                .unwrap_or_default()
                .trim()
                .to_owned())
        };
        let port = row
            .get::<_, Option<i64>>(2)?
            .and_then(|p| u16::try_from(p).ok())
            .filter(|p| *p != 0)
            .unwrap_or(DEFAULT_PORT);
        Ok(Favourite {
            name: text(0)?,
            host: text(1)?,
            port,
            username: text(3)?,
            // Not trimmed: spaces can be part of a password.
            password: row.get::<_, Option<String>>(4)?.filter(|p| !p.is_empty()),
        })
    })?;
    let mut favourites = Vec::new();
    for row in rows {
        let favourite = row?;
        if !favourite.host.is_empty() {
            favourites.push(favourite);
        }
    }
    Ok(favourites)
}

/// SQLite URIs need `%`, `?` and `#` escaped in the path.
fn percent_encode(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        match c {
            '%' | '?' | '#' => out.push_str(&format!("%{:02X}", u32::from(c))),
            // Windows paths work with forward slashes in a URI.
            '\\' => out.push('/'),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Test helper: a database with Mumble 1.5's `servers` table.
    pub(crate) fn write_db(path: &Path, rows: &[(&str, &str, i64, &str, Option<&str>)]) {
        let conn = Connection::open(path).expect("open");
        conn.execute_batch(
            "CREATE TABLE servers (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, \
             hostname TEXT, port INTEGER DEFAULT 64738, username TEXT, password TEXT, url TEXT);",
        )
        .expect("schema");
        for (name, host, port, user, password) in rows {
            conn.execute(
                "INSERT INTO servers (name, hostname, port, username, password) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![name, host, port, user, password],
            )
            .expect("insert");
        }
    }

    #[test]
    fn favourites_are_read_without_changing_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mumble #1?.sqlite");
        write_db(
            &path,
            &[
                ("Retro LAN", "retro.example", 64738, "zoe", Some("s3cret ")),
                ("Coffee Club", "coffee.example", 0, "", None),
                ("Broken", "", 1, "x", None),
            ],
        );
        let before = std::fs::read(&path).expect("read");
        let favourites = read(&path).expect("favourites");
        assert_eq!(
            favourites,
            vec![
                Favourite {
                    name: "Coffee Club".into(),
                    host: "coffee.example".into(),
                    port: DEFAULT_PORT,
                    username: String::new(),
                    password: None,
                },
                Favourite {
                    name: "Retro LAN".into(),
                    host: "retro.example".into(),
                    port: 64738,
                    username: "zoe".into(),
                    password: Some("s3cret ".into()),
                },
            ]
        );
        assert_eq!(std::fs::read(&path).expect("read"), before);
        assert!(!format!("{favourites:?}").contains("s3cret"));
    }

    #[test]
    fn a_database_without_favourites_table_is_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("empty.sqlite");
        Connection::open(&path).expect("open");
        assert!(read(&path).is_err());
    }
}
