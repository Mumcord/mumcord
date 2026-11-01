//! `new`: creates a plugin project. The template is embedded so it works offline and always
//! matches this CLI and SDK version; `Mumcord/plugins/template/` holds the output of this command
//! (lead decision for B25b/B38).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use mumcord_plugins::manifest;
use serde_json::json;

const SDK_GIT: &str = "https://github.com/Mumcord/mumcord";
const SDK_BRANCH: &str = "epic/plugins";

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

pub fn run(id: &str, dir: Option<PathBuf>, sdk_path: Option<&Path>) -> Result<()> {
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
    for (path, body) in files(id, sdk_path.as_deref())? {
        let path = dir.join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, body)?;
    }
    println!("created {}", dir.display());
    Ok(())
}

fn files(id: &str, sdk_path: Option<&Path>) -> Result<Vec<(&'static str, String)>> {
    let name = title(id);
    let sdk = match sdk_path {
        Some(path) => format!(
            "{{ path = {} }}",
            toml::Value::from(path.display().to_string())
        ),
        None => format!("{{ git = \"{SDK_GIT}\", branch = \"{SDK_BRANCH}\" }}"),
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
    let manifest = json!({
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
    });
    let english = json!({
        "manifest.name": name,
        "manifest.description": "Wave at the channel.",
        "heading": "Hello",
        "wave": "Wave",
        "waves_one": "{{count}} wave",
        "waves_other": "{{count}} waves",
    });
    Ok(vec![
        ("Cargo.toml", cargo),
        ("src/lib.rs", LIB_RS.to_owned()),
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
    ])
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
        run("wave-demo", Some(project.clone()), None).expect("test step");

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
            run("wave-demo", Some(project), None).is_err(),
            "never overwrites a project"
        );
        assert!(run("Bad_Id", Some(dir.path().join("x")), None).is_err());
        assert!(run("core-thing", Some(dir.path().join("y")), None).is_err());
    }

    #[test]
    fn local_sdk_path_is_quoted() {
        let files = files("abc", Some(Path::new("/tmp/my sdk"))).expect("test step");
        assert!(
            files[0]
                .1
                .contains("mumcord-plugin-sdk = { path = \"/tmp/my sdk\" }")
        );
    }
}
