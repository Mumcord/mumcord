//! The plugin API package `mumcord:plugin@1.0.0` is the frozen contract for the runtime, the SDK,
//! the UI renderer and every plugin. These tests pin its shape to the spec, so an accidental
//! change fails here instead of breaking published plugins.

use std::path::Path;

use wit_parser::{Resolve, TypeDefKind, TypeId, WorldItem, WorldKey};

fn package() -> (Resolve, wit_parser::PackageId) {
    let mut resolve = Resolve::default();
    let (package, _) = resolve
        .push_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("wit"))
        .expect("WIT parses");
    (resolve, package)
}

fn type_in(resolve: &Resolve, interface: &str, name: &str) -> TypeId {
    let (_, iface) = resolve
        .interfaces
        .iter()
        .find(|(_, i)| i.name.as_deref() == Some(interface))
        .unwrap_or_else(|| panic!("interface {interface} exists"));
    *iface
        .types
        .get(name)
        .unwrap_or_else(|| panic!("type {interface}.{name} exists"))
}

fn case_names(resolve: &Resolve, id: TypeId) -> Vec<String> {
    match &resolve.types[id].kind {
        TypeDefKind::Variant(v) => v.cases.iter().map(|c| c.name.clone()).collect(),
        TypeDefKind::Enum(e) => e.cases.iter().map(|c| c.name.clone()).collect(),
        other => panic!("not a variant or enum: {other:?}"),
    }
}

#[test]
fn package_is_mumcord_plugin_1_0_0() {
    let (resolve, package) = package();
    let name = &resolve.packages[package].name;
    assert_eq!(name.to_string(), "mumcord:plugin@1.0.0");
}

#[test]
fn world_imports_every_host_interface_and_exports_the_lifecycle() {
    let (resolve, package) = package();
    let world = resolve
        .select_world(&[package], Some("plugin"))
        .expect("world exists");
    let world = &resolve.worlds[world];

    let mut imports: Vec<&str> = world
        .imports
        .iter()
        .filter_map(|(key, item)| match (key, item) {
            (WorldKey::Interface(id), WorldItem::Interface { .. }) => {
                resolve.interfaces[*id].name.as_deref()
            }
            _ => None,
        })
        .collect();
    imports.sort_unstable();
    assert_eq!(
        imports,
        [
            "chat", "clock", "events", "http", "log", "notify", "peers", "random", "secrets",
            "storage", "theme", "timers", "types", "ui", "webview"
        ]
    );

    let mut exports: Vec<String> = world
        .exports
        .keys()
        .map(|k| resolve.name_world_key(k))
        .collect();
    exports.sort_unstable();
    assert_eq!(exports, ["on-event", "start", "stop"]);
}

#[test]
fn errors_match_the_spec() {
    let (resolve, _) = package();
    let error = type_in(&resolve, "types", "error");
    assert_eq!(
        case_names(&resolve, error),
        [
            "not-permitted",
            "too-large",
            "rate-limited",
            "no-recipients",
            "hidden",
            "quota-exceeded",
            "invalid"
        ]
    );
}

#[test]
fn every_event_of_fr_6_16_exists() {
    let (resolve, _) = package();
    let event = type_in(&resolve, "events", "event");
    assert_eq!(
        case_names(&resolve, event),
        [
            "talking",
            "joined",
            "left",
            "screen-share-started",
            "chat-activity",
            "mentioned",
            "self-changed",
            "theme-changed",
            "language-changed",
            "settings-changed",
            "ui-action",
            "timer",
            "peer-message",
            "profile-card-opened",
            "panel-visibility-changed",
            "user-menu-requested",
            "dialog-closed",
            // Answers to asynchronous calls, which may not block for longer than 50 ms.
            "http-response",
            "webview-message",
            "chat-trigger",
            "chat-match",
            "peer-ready",
        ]
    );
}

#[test]
fn ui_has_every_slot_of_fr_6_40_and_component_of_fr_6_41() {
    let (resolve, _) = package();
    let slot = type_in(&resolve, "ui", "slot");
    assert_eq!(
        case_names(&resolve, slot),
        [
            "panel",
            "member-badge",
            "profile-card",
            "chat-card",
            "chat-decoration",
            "settings",
            "dialog"
        ]
    );
    let component = type_in(&resolve, "ui", "component");
    assert_eq!(
        case_names(&resolve, component),
        [
            "stack",
            "text",
            "image",
            "sprite",
            "meter",
            "button",
            "icon-button",
            "chip",
            "divider",
            "progress",
            "list",
            "toggle",
            "select",
            "slider",
            "text-input",
            "password",
            "file-picker",
            "countdown",
            "copy-button"
        ]
    );
    let animation = type_in(&resolve, "ui", "animation");
    assert_eq!(
        case_names(&resolve, animation),
        ["bob", "hop", "wave", "nap", "sparkle", "shake", "fade"]
    );
}

fn field_names(resolve: &Resolve, id: TypeId) -> Vec<String> {
    match &resolve.types[id].kind {
        TypeDefKind::Record(r) => r.fields.iter().map(|f| f.name.clone()).collect(),
        other => panic!("not a record: {other:?}"),
    }
}

fn has_function(resolve: &Resolve, interface: &str, name: &str) -> bool {
    resolve
        .interfaces
        .iter()
        .find(|(_, i)| i.name.as_deref() == Some(interface))
        .is_some_and(|(_, i)| i.functions.contains_key(name))
}

/// Bot info and the filtered user-menu entry were added to 1.0.0 additively: `peer` keeps its
/// fields, so plugins built before them still link.
#[test]
fn bot_info_and_filtered_user_menu_are_additive() {
    let (resolve, _) = package();
    let peer = type_in(&resolve, "types", "peer");
    assert_eq!(field_names(&resolve, peer), ["session", "name", "key"]);
    let bot = type_in(&resolve, "types", "bot-info");
    assert_eq!(field_names(&resolve, bot), ["name", "modules"]);
    let filter = type_in(&resolve, "ui", "user-menu-filter");
    assert_eq!(
        field_names(&resolve, filter),
        ["bot-name", "bot-module", "sessions"]
    );
    assert!(has_function(&resolve, "peers", "bot"));
    assert!(has_function(&resolve, "ui", "set-user-menu-entry"));
    assert!(has_function(&resolve, "ui", "set-user-menu-entry-for"));
}

/// The panel popover calls were added to 1.0.0 additively.
#[test]
fn panel_popover_calls_are_additive() {
    let (resolve, _) = package();
    assert!(has_function(&resolve, "ui", "open-panel-popover"));
    assert!(has_function(&resolve, "ui", "close-panel-popover"));
}

/// Snapshot of every WIT declaration without comments or indentation, one per line.
const SNAPSHOT: &str = "tests/fixtures/wit-1.0.0.snapshot";

fn declarations() -> String {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("wit");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("wit dir")
        .map(|entry| entry.expect("entry").path())
        .collect();
    files.sort();
    let mut out = String::new();
    for path in files {
        let name = path
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        out.push_str(&format!("== {name}\n"));
        for line in std::fs::read_to_string(&path).expect("read wit").lines() {
            let line = line.trim();
            if !line.is_empty() && !line.starts_with("//") {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

/// Plugins built against one shape of the package do not load against another: wasmtime checks
/// the case counts of enums and variants and the fields of records exactly (see
/// `ui_types_must_match_the_host_exactly` in runtime.rs). So after the 0.6.0 release every
/// declaration is frozen; doc comments may still change. Before that release the lead may
/// approve a change, then refresh the snapshot with `UPDATE_WIT_SNAPSHOT=1`.
#[test]
fn package_shape_is_frozen() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(SNAPSHOT);
    let now = declarations();
    if std::env::var_os("UPDATE_WIT_SNAPSHOT").is_some() {
        std::fs::write(&path, &now).expect("write snapshot");
        return;
    }
    // A Windows checkout may turn the line ends into CRLF.
    let pinned = std::fs::read_to_string(&path)
        .expect("snapshot exists")
        .replace("\r\n", "\n");
    assert!(
        pinned == now,
        "WIT mumcord:plugin@1.0.0 changed. It is frozen at the 0.6.0 release: adding enum or \
         variant cases or record fields breaks every plugin built before (wasmtime checks exact \
         case counts). Later changes need a new package version (@1.1.0) linked alongside 1.0.0 \
         with host-side tree conversion (specs/06-plugin-system-specification.md). Compare \
         {SNAPSHOT} with the WIT files to see what changed."
    );
}
