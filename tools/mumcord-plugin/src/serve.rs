//! `serve-registry`: a local fixture registry for development builds and E2E tests (FR-6.71,
//! AC-6.18 … AC-6.22, AC-6.28). It signs everything with a test key and serves it over plain HTTP
//! on 127.0.0.1 only. Scenario switches produce the broken registries the client must refuse; they
//! can be set at start and changed while running:
//!
//! - `POST /_ctl/scenario?set=<s1>,<s2>` replaces the active scenarios (empty clears them),
//! - `POST /_ctl/reload` rescans the package folder and raises the sequence ("publish").
//!
//! Minimal std::net server on purpose (lead decision): no new dependency for a test tool.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use anyhow::{Context, Result, bail};
use mumcord_plugins::package::Digest;
use mumcord_plugins::registry::{
    IndexPlugin, KeyRotation, RepositoryInfo, Revocation, SIGNATURE_SUFFIX, Severity,
    key_fingerprint,
};

use crate::date;
use crate::registry::{self, PackageInfo};
use crate::sign::Signer;

#[derive(clap::Args)]
pub struct Args {
    /// Folder with the `.mcplugin` packages to list.
    #[arg(long)]
    pub packages: PathBuf,
    /// Secret test key that signs the index (and `revoked.json`). Never a real registry key.
    #[arg(long)]
    pub key: PathBuf,
    /// Secret key that signs the packages as their author (default: the registry key).
    #[arg(long)]
    pub author_key: Option<PathBuf>,
    /// Port on 127.0.0.1 (0 picks a free one).
    #[arg(long, default_value_t = 0)]
    pub port: u16,
    /// Serve a third-party repository with this name instead of the official registry.
    #[arg(long)]
    pub repository: Option<String>,
    /// First index sequence (default: the current Unix time, so restarts never go backwards).
    #[arg(long)]
    pub sequence: Option<u64>,
    /// tampered-hash:<id>, lower-sequence, wrong-key, extra-permission:<id>,
    /// revoke:<id>[@<version>][:warn], key-change[:rotate]. Repeatable.
    #[arg(long = "scenario")]
    pub scenarios: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Scenario {
    /// The index lists a wrong SHA-256 for every version of the plugin.
    TamperedHash(String),
    /// The index carries a sequence one below the current one.
    LowerSequence,
    /// The index is signed with a key nobody trusts.
    WrongKey,
    /// The index allows one permission less than the plugin's manifest asks for.
    ExtraPermission(String),
    Revoke {
        id: String,
        version: Option<String>,
        severity: Severity,
    },
    /// The index is signed with a second key; `rotate` adds a `key_rotation` signed by the old one.
    KeyChange { rotate: bool },
}

impl Scenario {
    fn parse(text: &str) -> Result<Self> {
        let (name, arg) = match text.split_once(':') {
            Some((name, arg)) => (name, Some(arg)),
            None => (text, None),
        };
        Ok(match (name, arg) {
            ("tampered-hash", Some(id)) => Self::TamperedHash(id.to_owned()),
            ("lower-sequence", None) => Self::LowerSequence,
            ("wrong-key", None) => Self::WrongKey,
            ("extra-permission", Some(id)) => Self::ExtraPermission(id.to_owned()),
            ("revoke", Some(arg)) => {
                let (target, severity) = match arg.strip_suffix(":warn") {
                    Some(target) => (target, Severity::Warn),
                    None => (arg, Severity::Disable),
                };
                let (id, version) = match target.split_once('@') {
                    Some((id, version)) => (id, Some(version.to_owned())),
                    None => (target, None),
                };
                Self::Revoke {
                    id: id.to_owned(),
                    version,
                    severity,
                }
            }
            ("key-change", None) => Self::KeyChange { rotate: false },
            ("key-change", Some("rotate")) => Self::KeyChange { rotate: true },
            _ => bail!("unknown scenario {text}"),
        })
    }
}

fn parse_scenarios<'a>(items: impl IntoIterator<Item = &'a str>) -> Result<Vec<Scenario>> {
    items
        .into_iter()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(Scenario::parse)
        .collect()
}

struct Registry {
    packages: PathBuf,
    repository: Option<String>,
    base: String,
    signer: Signer,
    author: Signer,
    /// Signs the index in `wrong-key`.
    stranger: Signer,
    /// The repository's next key in `key-change`.
    next_key: Signer,
    sequence: u64,
    scenarios: Vec<Scenario>,
    files: HashMap<String, Vec<u8>>,
}

impl Registry {
    fn has(&self, scenario: &Scenario) -> bool {
        self.scenarios.contains(scenario)
    }

    fn key_change(&self) -> Option<bool> {
        self.scenarios.iter().find_map(|s| match s {
            Scenario::KeyChange { rotate } => Some(*rotate),
            _ => None,
        })
    }

    /// Rebuilds every served file from the package folder and the active scenarios.
    fn render(&mut self) -> Result<()> {
        let mut files = HashMap::new();
        let mut by_id: BTreeMap<String, Vec<PackageInfo>> = BTreeMap::new();
        let mut paths: Vec<PathBuf> = fs::read_dir(&self.packages)
            .with_context(|| format!("could not read {}", self.packages.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "mcplugin"))
            .collect();
        paths.sort();
        for path in paths {
            let info = PackageInfo::read(fs::read(&path)?)
                .with_context(|| format!("could not read {}", path.display()))?;
            let name = format!("packages/{}", info.file_name());
            files.insert(
                format!("{name}{SIGNATURE_SUFFIX}"),
                self.author.sign(&info.bytes)?.into_bytes(),
            );
            files.insert(name, info.bytes.clone());
            by_id
                .entry(info.manifest.id.clone())
                .or_default()
                .push(info);
        }

        let mut plugins = Vec::new();
        for (id, infos) in by_id {
            let mut plugin = self.index_plugin(&id, &infos)?;
            if let Some(png) = latest_package(&infos).and_then(|i| i.icon_png.clone()) {
                files.insert(format!("v1/media/{id}/icon.png"), png);
            }
            if self.has(&Scenario::TamperedHash(id.clone())) {
                for version in &mut plugin.versions {
                    version.sha256 = tamper(&version.sha256);
                }
            }
            if self.has(&Scenario::ExtraPermission(id.clone())) {
                for version in &mut plugin.versions {
                    if version.permissions.pop().is_none() {
                        bail!(
                            "extra-permission:{id}: the plugin asks for no permission to leave out"
                        );
                    }
                }
            }
            plugins.push(plugin);
        }

        let sequence = if self.has(&Scenario::LowerSequence) {
            self.sequence.saturating_sub(1)
        } else {
            self.sequence
        };
        let key_change = self.key_change();
        let index_signer = if self.has(&Scenario::WrongKey) {
            &self.stranger
        } else if key_change.is_some() {
            &self.next_key
        } else {
            &self.signer
        };
        let repository = self.repository.as_ref().map(|name| RepositoryInfo {
            name: name.clone(),
            homepage: None,
            contact: None,
            public_key: if key_change.is_some() {
                self.next_key.public_line()
            } else {
                self.signer.public_line()
            },
        });
        let rotation = match key_change {
            Some(true) => {
                let new_key = self.next_key.public_line();
                let signature = self.signer.sign(new_key.as_bytes())?;
                Some(KeyRotation { new_key, signature })
            }
            _ => None,
        };
        let index =
            registry::index_json(sequence, repository.as_ref(), rotation.as_ref(), &plugins)?;
        files.insert(
            format!("v1/index.json{SIGNATURE_SUFFIX}"),
            index_signer.sign(&index)?.into_bytes(),
        );
        files.insert("v1/index.json".to_owned(), index);

        if self.repository.is_none() {
            let revoked = registry::revoked_json(self.sequence, &self.revocations())?;
            files.insert(
                format!("v1/revoked.json{SIGNATURE_SUFFIX}"),
                self.signer.sign(&revoked)?.into_bytes(),
            );
            files.insert("v1/revoked.json".to_owned(), revoked);
            files.insert(
                "v1/permissions.json".to_owned(),
                registry::permissions_json()?,
            );
        }
        self.files = files;
        Ok(())
    }

    fn index_plugin(&self, id: &str, infos: &[PackageInfo]) -> Result<IndexPlugin> {
        let latest = latest_package(infos).context("no package")?;
        let manifest = &latest.manifest;
        let manifest_icon_url = latest
            .icon_png
            .is_some()
            .then(|| format!("{}v1/media/{id}/icon.png", self.base));
        let versions = infos
            .iter()
            .map(|info| info.index_version(format!("{}packages/{}", self.base, info.file_name())))
            .collect::<Result<_>>()?;
        Ok(IndexPlugin {
            id: id.to_owned(),
            name: BTreeMap::from([("en".to_owned(), manifest.name.clone())]),
            summary: BTreeMap::from([("en".to_owned(), manifest.description.clone())]),
            description_md_url: None,
            icon: manifest_icon_url,
            screenshots: Vec::new(),
            categories: manifest.categories.clone(),
            authors: manifest.authors.clone(),
            author_keys: vec![self.author.public_line()],
            reviewed: self.repository.is_none(),
            source_url: Some(format!("{}plugins/{id}", self.base)),
            license: Some("MIT".to_owned()),
            min_mumcord: Some("0.7".to_owned()),
            reviewed_on: self.repository.is_none().then(date::today),
            added_on: Some(date::today()),
            updated: date::today(),
            versions,
        })
    }

    fn revocations(&self) -> Vec<Revocation> {
        self.scenarios
            .iter()
            .filter_map(|s| match s {
                Scenario::Revoke {
                    id,
                    version,
                    severity,
                } => Some(Revocation {
                    id: id.clone(),
                    versions: version.iter().cloned().collect(),
                    sha256: Vec::new(),
                    author_key: None,
                    severity: *severity,
                    reason: BTreeMap::from([
                        ("en".to_owned(), "Revoked for testing".to_owned()),
                        ("de".to_owned(), "Zum Testen gesperrt".to_owned()),
                        ("es".to_owned(), "Revocado para pruebas".to_owned()),
                    ]),
                    since: date::today(),
                }),
                _ => None,
            })
            .collect()
    }

    fn control(&mut self, path: &str, query: &str) -> Result<()> {
        match path {
            "/_ctl/scenario" => {
                let list = query
                    .split('&')
                    .find_map(|p| p.strip_prefix("set="))
                    .context("missing ?set=")?;
                self.scenarios = parse_scenarios(list.split(','))?;
            }
            "/_ctl/reload" => self.sequence = (self.sequence + 1).max(date::unix_seconds()),
            _ => bail!("unknown control path"),
        }
        self.render()
    }
}

/// Flips the first hex digit, so the hash stays well-formed but never matches.
fn latest_package(infos: &[PackageInfo]) -> Option<&PackageInfo> {
    infos
        .iter()
        .max_by(|a, b| a.manifest.version.cmp(&b.manifest.version))
}

fn tamper(sha256: &str) -> String {
    let mut chars: Vec<char> = sha256.chars().collect();
    if let Some(first) = chars.first_mut() {
        *first = if *first == '0' { '1' } else { '0' };
    }
    chars.into_iter().collect()
}

/// A running registry; dropping it does not stop the server thread (the process ends instead).
pub struct Running {
    /// `http://127.0.0.1:<port>/`
    pub base: String,
    pub registry_key: String,
}

impl Running {
    /// The index address (official base + `index.json`, or the third-party repository address).
    pub fn index_url(&self) -> String {
        format!("{}v1/index.json", self.base)
    }
}

pub fn start(args: Args) -> Result<Running> {
    let listener =
        TcpListener::bind(("127.0.0.1", args.port)).context("could not open the port")?;
    let base = format!("http://127.0.0.1:{}/", listener.local_addr()?.port());
    let signer = Signer::load(&args.key)?;
    let author = match &args.author_key {
        Some(path) => Signer::load(path)?,
        None => Signer::load(&args.key)?,
    };
    let mut registry = Registry {
        packages: args.packages,
        repository: args.repository,
        base: base.clone(),
        signer,
        author,
        stranger: Signer::generate()?,
        next_key: Signer::generate()?,
        sequence: args.sequence.unwrap_or_else(date::unix_seconds),
        scenarios: parse_scenarios(args.scenarios.iter().map(String::as_str))?,
        files: HashMap::new(),
    };
    registry.render()?;
    let registry_key = registry.signer.public_line();
    let registry = Arc::new(Mutex::new(registry));
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let registry = Arc::clone(&registry);
            thread::spawn(move || {
                if let Err(e) = handle(stream, &registry) {
                    eprintln!("request failed: {e:#}");
                }
            });
        }
    });
    Ok(Running { base, registry_key })
}

pub fn run(args: Args) -> Result<()> {
    let official = args.repository.is_none();
    let running = start(args)?;
    if official {
        println!("official registry base {}v1/", running.base);
        println!("set MUMCORD_PLUGIN_REGISTRY_KEY={}", running.registry_key);
    } else {
        println!("repository address {}", running.index_url());
        println!(
            "repository key {} (fingerprint {})",
            running.registry_key,
            key_fingerprint(&running.registry_key).unwrap_or_default()
        );
    }
    std::io::stdout().flush()?;
    loop {
        thread::park();
    }
}

fn handle(stream: TcpStream, registry: &Mutex<Registry>) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut if_none_match = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("if-none-match")
        {
            if_none_match = Some(value.trim().to_owned());
        }
    }
    let mut parts = request_line.split_whitespace();
    let (method, target) = (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
    );
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut registry = registry.lock().unwrap_or_else(PoisonError::into_inner);

    let response = match method {
        "POST" if path.starts_with("/_ctl/") => match registry.control(path, query) {
            Ok(()) => Response::text(200, "ok\n"),
            Err(e) => Response::text(400, &format!("{e:#}\n")),
        },
        "GET" | "HEAD" => match registry.files.get(path.trim_start_matches('/')) {
            Some(body) => {
                let etag = format!("\"{}\"", &Digest::of(body).sha256_hex()[..16]);
                if if_none_match.as_deref() == Some(etag.as_str()) {
                    Response {
                        status: 304,
                        etag: Some(etag),
                        body: Vec::new(),
                        content_type: "text/plain",
                    }
                } else {
                    Response {
                        status: 200,
                        etag: Some(etag),
                        body: body.clone(),
                        content_type: content_type(path),
                    }
                }
            }
            None => Response::text(404, "not found\n"),
        },
        _ => Response::text(405, "method not allowed\n"),
    };
    drop(registry);
    eprintln!("{method} {path} {}", response.status);
    response.write(stream, method == "HEAD")
}

struct Response {
    status: u16,
    etag: Option<String>,
    body: Vec<u8>,
    content_type: &'static str,
}

impl Response {
    fn text(status: u16, text: &str) -> Self {
        Self {
            status,
            etag: None,
            body: text.as_bytes().to_vec(),
            content_type: "text/plain",
        }
    }

    fn write(self, mut stream: TcpStream, head: bool) -> Result<()> {
        let reason = match self.status {
            200 => "OK",
            304 => "Not Modified",
            400 => "Bad Request",
            404 => "Not Found",
            _ => "Method Not Allowed",
        };
        let mut head_text = format!(
            "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\nConnection: close\r\n",
            self.status,
            self.content_type,
            self.body.len()
        );
        if let Some(etag) = &self.etag {
            head_text.push_str(&format!("ETag: {etag}\r\n"));
        }
        head_text.push_str("\r\n");
        stream.write_all(head_text.as_bytes())?;
        if !head {
            stream.write_all(&self.body)?;
        }
        stream.flush()?;
        Ok(())
    }
}

fn content_type(path: &str) -> &'static str {
    if path.ends_with(".json") {
        "application/json"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".mcplugin") {
        "application/zip"
    } else {
        "text/plain; charset=utf-8"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_png_as_an_image() {
        assert_eq!(content_type("v1/media/dice/icon.png"), "image/png");
    }

    #[test]
    fn parses_scenarios() {
        assert_eq!(
            Scenario::parse("tampered-hash:dice").expect("ok"),
            Scenario::TamperedHash("dice".into())
        );
        assert_eq!(
            Scenario::parse("key-change:rotate").expect("ok"),
            Scenario::KeyChange { rotate: true }
        );
        assert_eq!(
            Scenario::parse("revoke:echolings@1.0.0:warn").expect("ok"),
            Scenario::Revoke {
                id: "echolings".into(),
                version: Some("1.0.0".into()),
                severity: Severity::Warn
            }
        );
        assert_eq!(
            Scenario::parse("revoke:echolings").expect("ok"),
            Scenario::Revoke {
                id: "echolings".into(),
                version: None,
                severity: Severity::Disable
            }
        );
        assert!(Scenario::parse("wrong-key:x").is_err());
        assert!(Scenario::parse("melt-down").is_err());
        assert!(parse_scenarios(["", " "]).expect("ok").is_empty());
    }

    #[test]
    fn tampering_keeps_the_hash_well_formed() {
        let hash = "0".repeat(64);
        let tampered = tamper(&hash);
        assert_ne!(tampered, hash);
        assert_eq!(tampered.len(), 64);
    }

    mod flow {
        use std::io::Read;

        use mumcord_plugins::manifest::Source;
        use mumcord_plugins::registry::{
            Client, Index, IndexKind, RevocationList, UrlPolicy, preview_repository,
        };
        use mumcord_plugins::verify::{self, RegistryKeys, VerifyFailure, check_sequence};

        use super::*;
        use crate::registry::tests::package;
        use crate::sign::{self, read_public};

        struct Fixture {
            _dir: tempfile::TempDir,
            key: PathBuf,
            public: String,
            packages: PathBuf,
        }

        fn fixture() -> Fixture {
            let dir = tempfile::tempdir().expect("test step");
            sign::keygen(dir.path(), "registry", true).expect("test step");
            let packages = dir.path().join("packages");
            fs::create_dir_all(&packages).expect("test step");
            for version in ["1.0.0", "1.1.0"] {
                fs::write(
                    packages.join(format!("dice-{version}.mcplugin")),
                    package("dice", version, &["chat.send", "random.bytes"]),
                )
                .expect("test step");
            }
            let public = read_public(dir.path().join("registry.pub").to_str().expect("utf-8"))
                .expect("test step");
            Fixture {
                key: dir.path().join("registry.key"),
                public,
                packages,
                _dir: dir,
            }
        }

        fn args(fixture: &Fixture, repository: Option<&str>) -> Args {
            Args {
                packages: fixture.packages.clone(),
                key: fixture.key.clone(),
                author_key: None,
                port: 0,
                repository: repository.map(str::to_owned),
                sequence: Some(10),
                scenarios: Vec::new(),
            }
        }

        /// Minimal HTTP/1.1 client: (status, ETag, body).
        fn request(
            base: &str,
            method: &str,
            path: &str,
            etag: Option<&str>,
        ) -> (u16, Option<String>, Vec<u8>) {
            let address = base.trim_start_matches("http://").trim_end_matches('/');
            let mut stream = TcpStream::connect(address).expect("test step");
            let extra = etag
                .map(|e| format!("If-None-Match: {e}\r\n"))
                .unwrap_or_default();
            write!(
                stream,
                "{method} /{path} HTTP/1.1\r\nHost: x\r\n{extra}\r\n"
            )
            .expect("test step");
            let mut raw = Vec::new();
            stream.read_to_end(&mut raw).expect("test step");
            let split = raw.windows(4).position(|w| w == b"\r\n\r\n").expect("head");
            let head = String::from_utf8_lossy(&raw[..split]).into_owned();
            let status = head[9..12].parse().expect("status");
            let etag = head
                .lines()
                .find_map(|l| l.strip_prefix("ETag: "))
                .map(str::to_owned);
            (status, etag, raw[split + 4..].to_vec())
        }

        fn get(base: &str, path: &str) -> Vec<u8> {
            let (status, _, body) = request(base, "GET", path, None);
            assert_eq!(status, 200, "{path}");
            body
        }

        fn switch(base: &str, scenarios: &str) {
            let (status, _, body) = request(
                base,
                "POST",
                &format!("_ctl/scenario?set={scenarios}"),
                None,
            );
            assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
        }

        fn official_index(base: &str, keys: &RegistryKeys) -> Result<Index, VerifyFailure> {
            let index = get(base, "v1/index.json");
            let signature = String::from_utf8(get(base, "v1/index.json.minisig")).expect("utf-8");
            keys.verify(&index, &signature)?;
            Ok(Index::parse(&index, &IndexKind::Official).expect("valid index"))
        }

        async fn install(
            client: &Client,
            index: &Index,
        ) -> Result<verify::Verified, VerifyFailure> {
            let plugin = index.plugin("dice").expect("listed");
            let latest = plugin.latest();
            let download = client
                .download(&latest.url, latest.size)
                .await
                .expect("download");
            verify::verify_package(
                &download.bytes,
                &plugin.entry(latest),
                &download.signature,
                Source::Official,
            )
        }

        #[tokio::test]
        async fn official_registry_and_scenarios_meet_the_client_checks() {
            let fixture = fixture();
            let running = start(args(&fixture, None)).expect("test step");
            let base = running.base.clone();
            let keys = RegistryKeys::pinned(&fixture.public).expect("test step");
            let client = Client::new(UrlPolicy::new(true)).expect("test step");

            let index = official_index(&base, &keys).expect("signed by the test key");
            assert_eq!(index.sequence, 10);
            let icon = index.plugin("dice").expect("listed").icon.clone();
            let icon = icon.expect("the index lists the package icon");
            assert_eq!(icon, format!("{base}v1/media/dice/icon.png"));
            assert_eq!(
                get(&base, "v1/media/dice/icon.png"),
                registry::tests::TEST_PNG
            );
            assert_eq!(
                index.plugin("dice").expect("listed").latest().version,
                "1.1.0"
            );
            let verified = install(&client, &index).await.expect("installs");
            assert!(verified.official);

            let (_, etag, _) = request(&base, "GET", "v1/index.json", None);
            let (status, _, _) = request(&base, "GET", "v1/index.json", etag.as_deref());
            assert_eq!(status, 304);
            assert!(!get(&base, "v1/permissions.json").is_empty());

            switch(&base, "tampered-hash:dice");
            let index = official_index(&base, &keys).expect("still signed");
            assert!(matches!(
                install(&client, &index).await,
                Err(VerifyFailure::Mismatch)
            ));

            switch(&base, "extra-permission:dice");
            let index = official_index(&base, &keys).expect("still signed");
            assert!(matches!(
                install(&client, &index).await,
                Err(VerifyFailure::Permissions)
            ));

            switch(&base, "lower-sequence");
            let index = official_index(&base, &keys).expect("still signed");
            assert!(matches!(
                check_sequence(index.sequence, Some(10)),
                Err(VerifyFailure::Rollback)
            ));

            switch(&base, "wrong-key");
            assert!(matches!(
                official_index(&base, &keys),
                Err(VerifyFailure::Signature)
            ));

            switch(&base, "revoke:dice@1.0.0");
            let revoked = get(&base, "v1/revoked.json");
            let signature =
                String::from_utf8(get(&base, "v1/revoked.json.minisig")).expect("utf-8");
            keys.verify(&revoked, &signature).expect("signed");
            let list = RevocationList::parse(&revoked).expect("valid");
            assert!(list.find("dice", "1.0.0", "", None).is_some());
            assert!(list.find("dice", "1.1.0", "", None).is_none());

            switch(&base, "");
            let (status, _, _) = request(&base, "POST", "_ctl/reload", None);
            assert_eq!(status, 200);
            let index = official_index(&base, &keys).expect("signed");
            assert!(index.sequence > 10, "a reload publishes a newer index");
            let (status, _, _) = request(&base, "POST", "_ctl/scenario?set=melt-down", None);
            assert_eq!(status, 400);
        }

        #[tokio::test]
        async fn third_party_repository_and_key_change() {
            let fixture = fixture();
            let running = start(args(&fixture, Some("Test Repo"))).expect("test step");
            let client = Client::new(UrlPolicy::new(true)).expect("test step");
            let url = running.index_url();

            let preview = preview_repository(&client, &url, None, &|id| id == "poll")
                .await
                .expect("valid repository");
            assert_eq!(preview.info.name, "Test Repo");
            assert_eq!(Some(preview.fingerprint), key_fingerprint(&fixture.public));
            assert_eq!(preview.plugin_count, 1);
            let (status, _, _) = request(&running.base, "GET", "v1/revoked.json", None);
            assert_eq!(
                status, 404,
                "only the official registry has a revocation list"
            );

            switch(&running.base, "key-change");
            assert!(
                preview_repository(&client, &url, Some(&fixture.public), &|_| false)
                    .await
                    .is_err(),
                "the expected (old) key no longer matches"
            );

            switch(&running.base, "key-change:rotate");
            let index: serde_json::Value =
                serde_json::from_slice(&get(&running.base, "v1/index.json")).expect("json");
            let new_key = index["key_rotation"]["new_key"].as_str().expect("rotation");
            assert_eq!(index["repository"]["public_key"], new_key);
            let signature = index["key_rotation"]["signature"]
                .as_str()
                .expect("signature");
            RegistryKeys::pinned(&fixture.public)
                .expect("test step")
                .verify(new_key.as_bytes(), signature)
                .expect("signed by the old key");
        }
    }
}
