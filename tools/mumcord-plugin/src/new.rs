//! `new`: creates a plugin project. The template is embedded so it works offline and always
//! matches this CLI and SDK version; `Mumcord/plugins/template/` holds the output of this command
//! (lead decision for B25b/B38).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use mumcord_plugins::manifest;
use serde_json::json;

/// The SDK is released together with this tool, so a new project asks for the SDK of the same
/// minor series from crates.io.
const SDK_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION_MAJOR"),
    ".",
    env!("CARGO_PKG_VERSION_MINOR")
);

const LIB_RS: &str = r#"//! A small plugin: a panel with a wave button that counts waves, stores the count and tells the
//! other users in the channel.

use std::cell::RefCell;

use mumcord_plugin_sdk::{
    Plugin,
    events::Event,
    i18n::Strings,
    log,
    peers::{self, Target},
    storage::{self, Scope},
    types::StartInfo,
    ui::{self, Animation, Slot},
};

const WAVE: u8 = 1;
const COUNT_KEY: &str = "waves";

struct Hello;

thread_local! {
    // Embeds i18n/en.json. Add a language by adding i18n/<tag>.json and its tag here.
    static STRINGS: RefCell<Strings> =
        RefCell::new(Strings::new(mumcord_plugin_sdk::i18n_embed!["en"]));
}

fn waves() -> u32 {
    storage::get_json(Scope::Identity, COUNT_KEY)
        .unwrap_or_else(|e| {
            log::warn(&format!("could not read the wave count: {e}"));
            None
        })
        .unwrap_or(0)
}

fn render(waves: u32) {
    let (heading, count, button) = STRINGS.with_borrow(|s| {
        (s.t("heading"), s.tn("waves", u64::from(waves), &[]), s.t("wave"))
    });
    let tree = ui::column()
        .child(ui::heading(&heading))
        .child(ui::text(&count))
        .child(ui::button("wave", &button).animate(Animation::Wave));
    if let Err(e) = ui::show(Slot::Panel, tree) {
        log::error(&format!("could not show the panel: {e}"));
    }
}

impl Plugin for Hello {
    fn start(_info: StartInfo) -> Result<(), String> {
        render(waves());
        Ok(())
    }

    fn on_event(event: Event) {
        match event {
            // The host sends the current language right after start and on every change.
            Event::LanguageChanged(tag) => {
                STRINGS.with_borrow_mut(|s| s.set_language(&tag));
                render(waves());
            }
            Event::UiAction(action) if action.action == "wave" => {
                let count = waves().saturating_add(1);
                if let Err(e) = storage::set_json(Scope::Identity, COUNT_KEY, &count) {
                    log::warn(&format!("could not store the wave count: {e}"));
                }
                // Nobody else in the channel is a normal case, not an error worth logging.
                let _ = peers::send_json(Target::Channel, WAVE, &());
                render(count);
            }
            Event::PeerMessage(message) if message.kind == WAVE => {
                log::info("someone waved");
            }
            _ => {}
        }
    }

    fn stop() {}
}

mumcord_plugin_sdk::export!(Hello);
"#;

const STAGE_TREE_LIB_RS: &str = r#"//! A stage plugin with tree content: an Apps-menu item starts a click counter in the media area.
//! The stage is drawn from building blocks, so no web page is needed.

use std::cell::{Cell, RefCell};

use mumcord_plugin_sdk::{Plugin, events::Event, i18n::Strings, log, stage, types::StartInfo, ui};

const START_ITEM: &str = "start";

struct Counter;

thread_local! {
    // Embeds i18n/en.json. Add a language by adding i18n/<tag>.json and its tag here.
    static STRINGS: RefCell<Strings> =
        RefCell::new(Strings::new(mumcord_plugin_sdk::i18n_embed!["en"]));
    static CLICKS: Cell<u32> = const { Cell::new(0) };
    static RUNNING: Cell<bool> = const { Cell::new(false) };
}

fn board() -> ui::El {
    let clicks = CLICKS.get();
    STRINGS.with_borrow(|s| {
        ui::column()
            .child(ui::heading(&s.t("heading")))
            .child(ui::text(&s.tn("clicks", u64::from(clicks), &[])))
            .child(ui::button("click", &s.t("click")))
    })
}

impl Plugin for Counter {
    fn start(_info: StartInfo) -> Result<(), String> {
        Ok(())
    }

    fn on_event(event: Event) {
        match event {
            Event::LanguageChanged(tag) => {
                STRINGS.with_borrow_mut(|s| s.set_language(&tag));
                if RUNNING.get() {
                    if let Err(e) = stage::show_tree(board()) {
                        log::warn(&format!("could not redraw the stage: {e:?}"));
                    }
                }
            }
            // Picking the item counts as the user gesture that `stage::start_tree` needs.
            Event::AppMenuActivated(item) if item == START_ITEM => {
                CLICKS.set(0);
                let title = STRINGS.with_borrow(|s| s.t("title"));
                match stage::start_tree(&title, board()) {
                    Ok(_) => RUNNING.set(true),
                    // For example: not in a voice channel, or the channel has too many activities.
                    Err(e) => log::warn(&format!("could not start the activity: {e:?}")),
                }
            }
            Event::UiAction(action) if action.action == "click" => {
                CLICKS.set(CLICKS.get().saturating_add(1));
                if let Err(e) = stage::show_tree(board()) {
                    log::warn(&format!("could not redraw the stage: {e:?}"));
                }
            }
            // Also fires when the user leaves the channel or disconnects.
            Event::StageActivityEnded(_) => RUNNING.set(false),
            _ => {}
        }
    }

    fn stop() {
        if RUNNING.get() {
            stage::end();
        }
    }
}

mumcord_plugin_sdk::export!(Counter);
"#;

const STAGE_WEB_LIB_RS: &str = r#"//! A stage plugin with web content: an Apps-menu item starts an activity that shows
//! `web/index.html` in the media area. The page and the plugin talk through the message bridge.

use std::cell::{Cell, RefCell};

use mumcord_plugin_sdk::{Plugin, events::Event, i18n::Strings, log, stage, types::StartInfo, webview};

const START_ITEM: &str = "start";

struct Counter;

thread_local! {
    // Embeds i18n/en.json. Add a language by adding i18n/<tag>.json and its tag here.
    static STRINGS: RefCell<Strings> =
        RefCell::new(Strings::new(mumcord_plugin_sdk::i18n_embed!["en"]));
    static CLICKS: Cell<u32> = const { Cell::new(0) };
    static RUNNING: Cell<bool> = const { Cell::new(false) };
}

/// Sends the count to the page. The page is untrusted code in a sandbox, so the plugin keeps the
/// real state and the page only displays it.
fn tell_page() {
    if let Err(e) = webview::post(&CLICKS.get().to_string()) {
        log::warn(&format!("could not message the page: {e:?}"));
    }
}

impl Plugin for Counter {
    fn start(_info: StartInfo) -> Result<(), String> {
        Ok(())
    }

    fn on_event(event: Event) {
        match event {
            Event::LanguageChanged(tag) => {
                STRINGS.with_borrow_mut(|s| s.set_language(&tag));
            }
            // Picking the item counts as the user gesture that `stage::start_web` needs.
            Event::AppMenuActivated(item) if item == START_ITEM => {
                CLICKS.set(0);
                let title = STRINGS.with_borrow(|s| s.t("title"));
                match stage::start_web(&title, "index.html") {
                    Ok(_) => RUNNING.set(true),
                    // For example: not in a voice channel, or the channel has too many activities.
                    Err(e) => log::warn(&format!("could not start the activity: {e:?}")),
                }
            }
            // Messages from the page are untrusted: accept only what is expected.
            Event::WebviewMessage(message) if message == "click" => {
                CLICKS.set(CLICKS.get().saturating_add(1));
                tell_page();
            }
            Event::WebviewMessage(message) if message == "ready" => tell_page(),
            Event::StageActivityEnded(_) => RUNNING.set(false),
            _ => {}
        }
    }

    fn stop() {
        if RUNNING.get() {
            stage::end();
        }
    }
}

mumcord_plugin_sdk::export!(Counter);
"#;

const STAGE_WEB_INDEX_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Click counter</title>
<style>
  :root { color-scheme: light dark; }
  body { font-family: system-ui, sans-serif; display: grid; place-items: center; min-height: 100vh; margin: 0; }
  main { text-align: center; }
  button { font: inherit; font-size: 1.25rem; padding: 0.75rem 1.5rem; }
</style>
</head>
<body>
<main>
  <h1>Click counter</h1>
  <p aria-live="polite">Clicks: <output id="count">0</output></p>
  <button id="click" type="button">Click</button>
</main>
<script>
  // The page has no network and no storage. It talks only to its plugin through the bridge:
  // strings in both directions.
  const count = document.getElementById("count");
  document.getElementById("click").addEventListener("click", () => window.parent.postMessage("click", "*"));
  window.addEventListener("message", (event) => {
    if (typeof event.data === "string" && /^[0-9]{1,9}$/.test(event.data)) count.textContent = event.data;
  });
  window.parent.postMessage("ready", "*");
</script>
</body>
</html>
"#;

const README: &str = "# {name}

A Mumcord plugin.

## Build

```sh
rustup target add wasm32-unknown-unknown
cargo generate-lockfile         # once; commit Cargo.lock
mumcord-plugin build            # writes dist/{id}-0.1.0.mcplugin
mumcord-plugin check dist/{id}-0.1.0.mcplugin
```

Commit `Cargo.lock`: `mumcord-plugin build --reproducible` and `mumcord-plugin test` need it.

## Try it in Mumcord

Switch on Developer mode, then use Developer → load on the plugins page. It needs an unpacked
folder (`manifest.json`, `plugin.wasm` and `i18n/en.json`), not a `.mcplugin` package.

## Licence

Add a `LICENSE` file with an OSI-approved licence before publishing.
";

/// Which project `new` creates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Template {
    /// A panel with a wave button (API 1.0).
    #[default]
    Hello,
    /// A stage activity drawn from building blocks (API 1.1).
    StageTree,
    /// A stage activity that shows a web page (API 1.1).
    StageWeb,
}

pub fn run(
    id: &str,
    dir: Option<PathBuf>,
    sdk_path: Option<&Path>,
    template: Template,
) -> Result<()> {
    if !manifest::is_valid_id(id) {
        bail!(
            "{id} is not a valid plugin id (a-z, 0-9 and -, 3 to 32 characters, starting with a letter)"
        );
    }
    if id.starts_with("mumcord-") || id.starts_with("core-") {
        bail!("the prefixes mumcord- and core- are reserved");
    }
    let dir = dir.unwrap_or_else(|| PathBuf::from(id));
    if dir.exists() && fs::read_dir(&dir)?.next().is_some() {
        bail!("{} already exists and is not empty", dir.display());
    }
    // Cargo resolves a relative path from the new project, not from where `new` was run.
    let sdk_path = sdk_path
        .map(|p| {
            p.canonicalize()
                .with_context(|| format!("{} does not exist", p.display()))
        })
        .transpose()?;
    for (path, body) in files(id, sdk_path.as_deref(), template)? {
        let path = dir.join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, body)?;
    }
    println!("created {}", dir.display());
    Ok(())
}

fn files(
    id: &str,
    sdk_path: Option<&Path>,
    template: Template,
) -> Result<Vec<(&'static str, String)>> {
    let name = title(id);
    let sdk = match sdk_path {
        Some(path) => format!(
            "{{ path = {} }}",
            toml::Value::from(path.display().to_string())
        ),
        None => format!("\"{SDK_VERSION}\""),
    };
    let sdk = if template == Template::Hello {
        sdk
    } else if sdk.starts_with('{') {
        // `{ path = "…" }` → `{ path = "…", features = ["1.1"] }`
        format!("{}, features = [\"1.1\"] }}", sdk.trim_end_matches(" }"))
    } else {
        format!("{{ version = {sdk}, features = [\"1.1\"] }}")
    };
    let cargo = format!(
        "[package]
name = \"{id}\"
version = \"0.1.0\"
edition = \"2024\"
license = \"Apache-2.0\"
publish = false

[lib]
crate-type = [\"cdylib\"]

[dependencies]
mumcord-plugin-sdk = {sdk}

[profile.release]
opt-level = \"s\"
lto = true
codegen-units = 1

# A plugin is its own workspace, also when it sits inside another project.
[workspace]
"
    );
    let (manifest, english, lib) = match template {
        Template::Hello => (
            json!({
                "manifest": 1,
                "id": id,
                "name": name,
                "description": "Wave at the channel.",
                "version": "0.1.0",
                "api": "^1.0",
                "authors": ["Your name"],
                "license": "Apache-2.0",
                "categories": ["fun"],
                "permissions": { "required": ["ui.panel", "peers.messages"] },
            }),
            json!({
                "manifest.name": name,
                "manifest.description": "Wave at the channel.",
                "heading": "Hello",
                "wave": "Wave",
                "waves_one": "{{count}} wave",
                "waves_other": "{{count}} waves",
            }),
            LIB_RS,
        ),
        Template::StageTree | Template::StageWeb => {
            let web = template == Template::StageWeb;
            let mut required = vec!["ui.stage", "ui.app-menu"];
            let mut manifest = json!({
                "manifest": 1,
                "id": id,
                "name": name,
                "description": "A click counter in the media area.",
                "version": "0.1.0",
                "api": "^1.1",
                "authors": ["Your name"],
                "license": "Apache-2.0",
                "categories": ["fun"],
                "app_menu": [{ "id": "start", "label": "Start counter" }],
                "stage": { "title": name, "max_participants": 8 },
            });
            if web {
                required.push("ui.webview");
                manifest["webview"] = json!({ "places": ["stage"] });
            }
            manifest["permissions"] = json!({ "required": required });
            let mut english = json!({
                "manifest.name": name,
                "manifest.description": "A click counter in the media area.",
                "title": name,
            });
            if !web {
                english["heading"] = "Click counter".into();
                english["click"] = "Click".into();
                english["clicks_one"] = "{{count}} click".into();
                english["clicks_other"] = "{{count}} clicks".into();
            }
            (
                manifest,
                english,
                if web {
                    STAGE_WEB_LIB_RS
                } else {
                    STAGE_TREE_LIB_RS
                },
            )
        }
    };
    let mut files = vec![
        ("Cargo.toml", cargo),
        ("src/lib.rs", lib.to_owned()),
        (
            "manifest.json",
            format!("{}\n", serde_json::to_string_pretty(&manifest)?),
        ),
        (
            "i18n/en.json",
            format!("{}\n", serde_json::to_string_pretty(&english)?),
        ),
        (
            "README.md",
            README.replace("{name}", &name).replace("{id}", id),
        ),
        (".gitignore", "/target\n/dist\n".to_owned()),
        // Pinned so that reproducible builds give the same hash on every machine.
        ("rust-toolchain.toml", TOOLCHAIN.to_owned()),
    ];
    if template == Template::StageWeb {
        files.push(("web/index.html", STAGE_WEB_INDEX_HTML.to_owned()));
    }
    Ok(files)
}

const TOOLCHAIN: &str = "[toolchain]
channel = \"1.99.1\"
targets = [\"wasm32-unknown-unknown\"]
profile = \"minimal\"
";

/// `my-plugin` → `My plugin`, cut to the 32-character name limit.
fn title(id: &str) -> String {
    let text = id.replace('-', " ");
    let mut chars = text.chars();
    let first = chars
        .next()
        .map(|c| c.to_ascii_uppercase())
        .unwrap_or_default();
    std::iter::once(first).chain(chars).take(32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mumcord_plugins::manifest::{Manifest, Source};

    #[test]
    fn creates_a_project_with_a_valid_manifest() {
        let dir = tempfile::tempdir().expect("test step");
        let project = dir.path().join("wave-demo");
        run("wave-demo", Some(project.clone()), None, Template::Hello).expect("test step");

        let manifest =
            Manifest::parse(&fs::read(project.join("manifest.json")).expect("test step"))
                .expect("valid manifest");
        manifest
            .validate(Source::Repo { repository_id: 0 })
            .expect("valid for third-party repos");
        assert_eq!(manifest.name, "Wave demo");
        let cargo: toml::Table = fs::read_to_string(project.join("Cargo.toml"))
            .expect("test step")
            .parse()
            .expect("valid Cargo.toml");
        assert_eq!(cargo["package"]["name"].as_str(), Some("wave-demo"));
        assert!(project.join("src/lib.rs").exists());
        let toolchain: toml::Table = fs::read_to_string(project.join("rust-toolchain.toml"))
            .expect("test step")
            .parse()
            .expect("valid rust-toolchain.toml");
        assert_eq!(toolchain["toolchain"]["channel"].as_str(), Some("1.99.1"));

        assert!(
            run("wave-demo", Some(project), None, Template::Hello).is_err(),
            "never overwrites a project"
        );
        assert!(run("Bad_Id", Some(dir.path().join("x")), None, Template::Hello).is_err());
        assert!(
            run(
                "core-thing",
                Some(dir.path().join("y")),
                None,
                Template::Hello
            )
            .is_err()
        );
    }

    #[test]
    fn stage_templates_have_valid_1_1_manifests() {
        for (template, web) in [(Template::StageTree, false), (Template::StageWeb, true)] {
            let dir = tempfile::tempdir().expect("test step");
            let project = dir.path().join("stage-demo");
            run("stage-demo", Some(project.clone()), None, template).expect("test step");

            let manifest =
                Manifest::parse(&fs::read(project.join("manifest.json")).expect("test step"))
                    .expect("valid manifest");
            manifest
                .validate(Source::Repo { repository_id: 0 })
                .expect("valid for third-party repos");
            assert_eq!(manifest.api_minor(), (1, 1));
            assert!(manifest.stage.is_some());
            assert_eq!(manifest.app_menu.len(), 1);
            assert_eq!(manifest.webview.is_some(), web);
            assert_eq!(project.join("web/index.html").exists(), web);
            let cargo: toml::Table = fs::read_to_string(project.join("Cargo.toml"))
                .expect("test step")
                .parse()
                .expect("valid Cargo.toml");
            let sdk = &cargo["dependencies"]["mumcord-plugin-sdk"];
            assert_eq!(sdk["features"][0].as_str(), Some("1.1"));
        }
    }

    #[test]
    fn stage_template_with_local_sdk_keeps_path_and_adds_feature() {
        let files =
            files("abc", Some(Path::new("/tmp/my sdk")), Template::StageTree).expect("test step");
        let cargo: toml::Table = files[0].1.parse().expect("valid Cargo.toml");
        let sdk = &cargo["dependencies"]["mumcord-plugin-sdk"];
        assert_eq!(sdk["path"].as_str(), Some("/tmp/my sdk"));
        assert_eq!(sdk["features"][0].as_str(), Some("1.1"));
    }

    #[test]
    fn sdk_comes_from_crates_io_by_default() {
        let files = files("abc", None, Template::Hello).expect("test step");
        let cargo: toml::Table = files[0].1.parse().expect("valid Cargo.toml");
        let wanted = format!(
            "{}.{}",
            env!("CARGO_PKG_VERSION_MAJOR"),
            env!("CARGO_PKG_VERSION_MINOR")
        );
        assert_eq!(
            cargo["dependencies"]["mumcord-plugin-sdk"].as_str(),
            Some(wanted.as_str())
        );
    }

    #[test]
    fn local_sdk_path_is_quoted() {
        let files =
            files("abc", Some(Path::new("/tmp/my sdk")), Template::Hello).expect("test step");
        assert!(
            files[0]
                .1
                .contains("mumcord-plugin-sdk = { path = \"/tmp/my sdk\" }")
        );
    }
}
