//! The plugin API packages `mumcord:plugin@1.0.0` (`wit/1.0.0`) and `@1.1.0` (`wit/1.1.0`) are
//! the frozen contract for the runtime, the SDK, the UI renderer and every plugin. These tests
//! pin their shape to the specs, so an accidental change fails here instead of breaking
//! published plugins.

use std::path::Path;

use wit_parser::{Resolve, TypeDefKind, TypeId, WorldItem, WorldKey};

fn wit_dir(version: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("wit")
        .join(version)
}

fn package_of(version: &str) -> (Resolve, wit_parser::PackageId) {
    let mut resolve = Resolve::default();
    let (package, _) = resolve.push_dir(wit_dir(version)).expect("WIT parses");
    (resolve, package)
}

fn package() -> (Resolve, wit_parser::PackageId) {
    package_of("1.0.0")
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

fn imports_and_exports(version: &str) -> (Vec<String>, Vec<String>) {
    let (resolve, package) = package_of(version);
    let world = resolve
        .select_world(&[package], Some("plugin"))
        .expect("world exists");
    let world = &resolve.worlds[world];

    let mut imports: Vec<String> = world
        .imports
        .iter()
        .filter_map(|(key, item)| match (key, item) {
            (WorldKey::Interface(id), WorldItem::Interface { .. }) => {
                resolve.interfaces[*id].name.clone()
            }
            _ => None,
        })
        .collect();
    imports.sort_unstable();
    let mut exports: Vec<String> = world
        .exports
        .keys()
        .map(|k| resolve.name_world_key(k))
        .collect();
    exports.sort_unstable();
    (imports, exports)
}

#[test]
fn world_imports_every_host_interface_and_exports_the_lifecycle() {
    let (imports, exports) = imports_and_exports("1.0.0");
    assert_eq!(
        imports,
        [
            "chat", "clock", "events", "http", "log", "notify", "peers", "random", "secrets",
            "storage", "theme", "timers", "types", "ui", "webview"
        ]
    );
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

fn snapshot_path(version: &str) -> String {
    format!("tests/fixtures/wit-{version}.snapshot")
}

/// Snapshot of every WIT declaration of one version without comments or indentation, one per
/// line.
fn declarations(version: &str) -> String {
    let dir = wit_dir(version);
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
/// `ui_types_must_match_the_host_exactly` in runtime.rs). So every published minor is frozen;
/// doc comments may still change. Before its release the lead may approve a change to the newest
/// minor, then refresh only that snapshot with `UPDATE_WIT_SNAPSHOT=<version>`.
fn assert_frozen(version: &str) {
    let snapshot = snapshot_path(version);
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(&snapshot);
    let now = declarations(version);
    if std::env::var("UPDATE_WIT_SNAPSHOT").is_ok_and(|v| v == version) {
        std::fs::write(&path, &now).expect("write snapshot");
        return;
    }
    // A Windows checkout may turn the line ends into CRLF.
    let pinned = std::fs::read_to_string(&path)
        .expect("snapshot exists")
        .replace("\r\n", "\n");
    assert!(
        pinned == now,
        "WIT mumcord:plugin@{version} changed. A published minor is frozen: adding enum or \
         variant cases or record fields breaks every plugin built against it (wasmtime checks \
         exact case counts). Changes need a new minor in wit/<version>/ linked alongside the \
         others with host-side conversion (FR-9.7, FR-9.8). Compare {snapshot} with the WIT \
         files to see what changed."
    );
}

#[test]
fn package_shape_is_frozen() {
    assert_frozen("1.0.0");
}

#[test]
fn package_1_1_shape_is_frozen() {
    assert_frozen("1.1.0");
}

fn type_in_1_1(interface: &str, name: &str) -> (Resolve, TypeId) {
    let (resolve, _) = package_of("1.1.0");
    let id = type_in(&resolve, interface, name);
    (resolve, id)
}

#[test]
fn package_1_1_is_mumcord_plugin_1_1_0() {
    let (resolve, package) = package_of("1.1.0");
    let name = &resolve.packages[package].name;
    assert_eq!(name.to_string(), "mumcord:plugin@1.1.0");
}

#[test]
fn world_1_1_imports_1_0_plus_the_new_interfaces() {
    let (imports, exports) = imports_and_exports("1.1.0");
    assert_eq!(
        imports,
        [
            "app-menu", "audio", "chat", "clock", "events", "files", "http", "log", "notify",
            "peers", "random", "relay", "secrets", "stage", "storage", "theme", "timers", "types",
            "ui", "webview"
        ]
    );
    assert_eq!(exports, ["on-event", "start", "stop"]);
}

/// FR-9.7: a minor only adds. Every declaration line of every 1.0 file appears in the same order
/// in its 1.1 counterpart, so new cases, fields and functions can only be inserted.
#[test]
fn minor_1_1_only_adds_to_1_0() {
    let old = declarations("1.0.0");
    let new = declarations("1.1.0");
    let files = |text: &str| -> Vec<(String, Vec<String>)> {
        text.split("== ")
            .filter(|chunk| !chunk.is_empty())
            .map(|chunk| {
                let mut lines = chunk.lines().map(str::to_owned);
                let name = lines.next().expect("file name");
                (name, lines.filter(|l| !l.starts_with("package ")).collect())
            })
            .collect()
    };
    let new_files = files(&new);
    for (name, old_lines) in files(&old) {
        let (_, new_lines) = new_files
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("1.1.0 drops {name}"));
        let mut rest = new_lines.iter();
        for line in &old_lines {
            assert!(
                rest.any(|l| l == line),
                "1.1.0 {name} changes or reorders the 1.0.0 declaration `{line}`"
            );
        }
    }
}

#[test]
fn errors_1_1_add_the_cases_of_fr_9_24() {
    let (resolve, error) = type_in_1_1("types", "error");
    assert_eq!(
        case_names(&resolve, error),
        [
            "not-permitted",
            "too-large",
            "rate-limited",
            "no-recipients",
            "hidden",
            "quota-exceeded",
            "invalid",
            "relay-unavailable",
            "origin-not-allowed",
            "not-found",
        ]
    );
}

#[test]
fn events_1_1_add_the_cases_of_fr_9_24() {
    let (resolve, event) = type_in_1_1("events", "event");
    let cases = case_names(&resolve, event);
    assert_eq!(cases.len(), 31);
    assert_eq!(
        cases[22..],
        [
            "stage-activity-started",
            "stage-activity-ended",
            "stage-joined",
            "stage-left",
            "stage-visibility-changed",
            "app-menu-activated",
            "relay-message",
            "relay-status-changed",
            "surface-closed",
        ]
    );
}

#[test]
fn webview_1_1_gains_side_panel_stage_and_size_hint() {
    let (resolve, place) = type_in_1_1("webview", "place");
    assert_eq!(
        case_names(&resolve, place),
        ["panel-popover", "dialog", "side-panel", "stage"]
    );
    assert!(has_function(&resolve, "webview", "set-size-hint"));
    let (resolve, slot) = type_in_1_1("ui", "slot");
    assert_eq!(
        case_names(&resolve, slot).last().map(String::as_str),
        Some("stage")
    );
}

#[test]
fn new_1_1_interfaces_have_the_functions_of_fr_9_24() {
    let (resolve, _) = package_of("1.1.0");
    let expected: [(&str, &[&str]); 4] = [
        ("stage", &["start", "join", "leave", "end", "set-tree"]),
        ("app-menu", &["set-state"]),
        ("relay", &["status", "send-channel", "send-direct"]),
        ("audio", &["play", "stop-all"]),
    ];
    for (interface, functions) in expected {
        for function in functions {
            assert!(
                has_function(&resolve, interface, function),
                "{interface}.{function} exists"
            );
        }
    }
    let content = type_in(&resolve, "stage", "stage-content");
    assert_eq!(case_names(&resolve, content), ["tree", "web"]);
    let status = type_in(&resolve, "relay", "relay-status");
    assert_eq!(
        case_names(&resolve, status),
        ["available", "not-allowed", "no-relay", "switched-off"]
    );
}

/// W1x addendum (FR-9.103 … FR-9.105, FR-9.107): Whiteboard's host-owned calls.
#[test]
fn files_and_post_image_are_in_1_1_only() {
    let (resolve, _) = package_of("1.1.0");
    assert!(has_function(&resolve, "files", "save"));
    assert!(has_function(&resolve, "files", "import-picture"));
    assert!(has_function(&resolve, "chat", "post-image"));
    let (resolve, picture) = type_in_1_1("files", "picture");
    assert_eq!(field_names(&resolve, picture), ["name", "data"]);
    let (resolve, outcome) = type_in_1_1("chat", "post-outcome");
    assert_eq!(case_names(&resolve, outcome), ["image", "file-offer"]);
    let (resolve, _) = package();
    assert!(!has_function(&resolve, "chat", "post-image"));
}
