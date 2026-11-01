//! `mumcord-plugin test` and `preview` with the real CLI and runtime, using a tiny hand-written
//! plugin (no wasm toolchain needed): it sends a peer message "hi" of kind 7 for every event.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PLUGIN: &str = r#"
(module
  (import "mumcord:plugin/peers@1.0.0" "send"
    (func $send (param i32 i32 i32 i32 i32 i32)))
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 4096))
  (data (i32.const 40) "hi")
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
    (local $p i32)
    (local.set $p (i32.and (i32.add (global.get $heap) (i32.sub (local.get 2) (i32.const 1)))
      (i32.sub (i32.const 0) (local.get 2))))
    (global.set $heap (i32.add (local.get $p) (local.get 3)))
    (local.get $p))
  (func (export "start") (param i32 i32 i32 i32 i32) (result i32)
    (i32.store8 (i32.const 64) (i32.const 0))
    (i32.const 64))
  (func (export "on-event") (param i32)
    (call $send (i32.const 0) (i32.const 0) (i32.const 7) (i32.const 40) (i32.const 2)
      (i32.const 100)))
  (func (export "stop"))
)"#;

fn plugin_dir(root: &Path) -> PathBuf {
    let manifest = r#"{"manifest":1,"id":"pinger","name":"Pinger","version":"1.0.0","api":"^1.0",
        "authors":["Tests"],"description":"Pings","categories":["tools"],"license":"MIT",
        "permissions":{"required":["peers.messages"]}}"#;
    build_plugin(root, "pinger", PLUGIN, manifest)
}

fn build_plugin(root: &Path, folder: &str, wat_source: &str, manifest: &str) -> PathBuf {
    let mut module = wat::parse_str(wat_source).expect("valid WAT");
    let mut resolve = wit_parser::Resolve::default();
    let (package, _) = resolve
        .push_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/plugins/wit"))
        .expect("WIT parses");
    let world = resolve
        .select_world(&[package], Some("plugin"))
        .expect("world exists");
    wit_component::embed_component_metadata(
        &mut module,
        &resolve,
        world,
        wit_component::StringEncoding::UTF8,
        false,
    )
    .expect("metadata embeds");
    let component = wit_component::ComponentEncoder::default()
        .validate(true)
        .module(&module)
        .expect("module is accepted")
        .encode()
        .expect("component encodes");

    let dir = root.join(folder);
    std::fs::create_dir_all(dir.join("i18n")).expect("mkdir");
    std::fs::write(dir.join("manifest.json"), manifest).expect("write manifest");
    std::fs::write(dir.join("plugin.wasm"), component).expect("write wasm");
    std::fs::write(dir.join("i18n/en.json"), "{}").expect("write i18n");
    dir
}

fn cli(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mumcord-plugin"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("the CLI runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn passing_and_failing_scenarios() {
    let work = tempfile::tempdir().expect("temp dir");
    let plugin = plugin_dir(work.path());
    let plugin = plugin.to_str().expect("utf-8");
    std::fs::write(
        work.path().join("pass.yaml"),
        "name: pings\npeers: [{session: 2, name: Bob}]\nsteps:\n\
         \x20 - event: {joined: 2}\n\
         \x20 - expect: {message: {to: channel, kind: 7, text: hi}}\n",
    )
    .expect("write");
    std::fs::write(
        work.path().join("fail.yaml"),
        "name: wrong kind\npeers: [{session: 2, name: Bob}]\nsteps:\n\
         \x20 - expect: {message: {kind: 8}}\n",
    )
    .expect("write");

    let passed = cli(work.path(), &["test", plugin, "pass.yaml"]);
    assert!(passed.status.success(), "{}", stdout(&passed));
    assert!(stdout(&passed).contains("ok   pings"));

    let failed = cli(work.path(), &["test", plugin, "pass.yaml", "fail.yaml"]);
    assert!(!failed.status.success());
    let out = stdout(&failed);
    assert!(out.contains("ok   pings"), "{out}");
    assert!(out.contains("FAIL wrong kind"), "{out}");
    assert!(
        out.contains("kind 7, payload 6869"),
        "the diff lists what came: {out}"
    );
}

#[test]
fn without_anyone_in_the_channel_nothing_is_sent() {
    let work = tempfile::tempdir().expect("temp dir");
    let plugin = plugin_dir(work.path());
    std::fs::write(
        work.path().join("alone.yaml"),
        "steps:\n  - event: {language: de}\n  - expect: nothing\n",
    )
    .expect("write");
    let output = cli(
        work.path(),
        &["test", plugin.to_str().expect("utf-8"), "alone.yaml"],
    );
    assert!(output.status.success(), "{}", stdout(&output));
}

#[test]
fn preview_refuses_a_plugin_without_ui() {
    let work = tempfile::tempdir().expect("temp dir");
    let plugin = plugin_dir(work.path());
    let output = cli(work.path(), &["preview", plugin.to_str().expect("utf-8")]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("shows no UI"));
}

/// Hides a trigger line, replaces a matched line by card "card-1" and tries a token it was
/// never given. Kind 5 / 6 says live / replay, 9 that the foreign token was refused, 7 that
/// `chat.current-message` was set outside a chat event.
const CARDS: &str = r#"
(module
  (import "mumcord:plugin/peers@1.0.0" "send" (func $send (param i32 i32 i32 i32 i32 i32)))
  (import "mumcord:plugin/chat@1.0.0" "current-message" (func $current (param i32)))
  (import "mumcord:plugin/chat@1.0.0" "is-replay" (func $replay (result i32)))
  (import "mumcord:plugin/chat@1.0.0" "hide" (func $hide (param i64 i32)))
  (import "mumcord:plugin/chat@1.0.0" "replace" (func $replace (param i64 i32 i32 i32)))
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 4096))
  (data (i32.const 40) "hi")
  (data (i32.const 48) "card-1")
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
    (local $p i32)
    (local.set $p (i32.and
      (i32.add (global.get $heap) (i32.sub (local.get 2) (i32.const 1)))
      (i32.sub (i32.const 0) (local.get 2))))
    (global.set $heap (i32.add (local.get $p) (local.get 3)))
    (local.get $p))
  (func (export "start") (param i32 i32 i32 i32 i32) (result i32)
    (i32.store8 (i32.const 64) (i32.const 0))
    (i32.const 64))
  (func (export "on-event") (param $event i32)
    (local $tag i32)
    (local.set $tag (i32.load8_u (local.get $event)))
    (call $current (i32.const 256))
    (if (i32.eq (local.get $tag) (i32.const 1))
      (then
        (if (i32.eq (i32.load8_u (i32.const 256)) (i32.const 1))
          (then (call $send (i32.const 0) (i32.const 0) (i32.const 7)
            (i32.const 40) (i32.const 2) (i32.const 100))))))
    (if (i32.eq (local.get $tag) (i32.const 19))
      (then (call $hide (i64.load (i32.const 264)) (i32.const 288))))
    (if (i32.eq (local.get $tag) (i32.const 20))
      (then
        (call $replace (i64.load (i32.const 264)) (i32.const 48) (i32.const 6) (i32.const 288))
        (call $send (i32.const 0) (i32.const 0)
          (i32.add (i32.const 5) (call $replay))
          (i32.const 40) (i32.const 2) (i32.const 100))
        (call $hide (i64.add (i64.load (i32.const 264)) (i64.const 1)) (i32.const 288))
        (if (i32.eq (i32.load8_u (i32.const 288)) (i32.const 1))
          (then (call $send (i32.const 0) (i32.const 0) (i32.const 9)
            (i32.const 40) (i32.const 2) (i32.const 100)))))))
  (func (export "stop"))
)"#;

#[test]
fn chat_tokens_replace_and_hide_messages() {
    let work = tempfile::tempdir().expect("temp dir");
    let manifest = r#"{"manifest":1,"id":"cards","name":"Cards","version":"1.0.0","api":"^1.0",
        "authors":["Tests"],"description":"Cards","categories":["tools"],"license":"MIT",
        "permissions":{"required":["peers.messages","chat.patterns","chat.triggers","ui.chat-card"]},
        "triggers":["!poll"],"chat_patterns":[{"pattern":"^\\[","description":"Fits"}]}"#;
    let plugin = build_plugin(work.path(), "cards", CARDS, manifest);
    let plugin = plugin.to_str().expect("utf-8");
    std::fs::write(
        work.path().join("cards.yaml"),
        "name: cards\npeers: [{session: 2, name: Bob}]\nsteps:\n\
         \x20 - event: {joined: 2}\n\
         \x20 - expect: nothing\n\
         \x20 - event: {chat-trigger: {sender: 2, trigger: '!poll', args: '2'}}\n\
         \x20 - expect: {hidden: {message: 1}}\n\
         \x20 - event: {chat-match: {sender: 2, text: '[Rifter]'}}\n\
         \x20 - expect: {replaced: {message: 2, card: card-1}}\n\
         \x20 - expect: {message: {kind: 5}}\n\
         \x20 - expect: {message: {kind: 9}}\n\
         \x20 - event: {chat-match: {sender: 2, text: '[Rifter]', message: 7, replay: true}}\n\
         \x20 - expect: {replaced: {message: 7}}\n\
         \x20 - expect: {message: {kind: 6}}\n\
         \x20 - expect: {message: {kind: 9}}\n\
         \x20 - expect: nothing\n",
    )
    .expect("write");
    std::fs::write(
        work.path().join("wrong.yaml"),
        "name: wrong message\npeers: [{session: 2, name: Bob}]\nsteps:\n\
         \x20 - event: {chat-trigger: {sender: 2, trigger: '!poll'}}\n\
         \x20 - expect: {hidden: {message: 3}}\n",
    )
    .expect("write");

    let passed = cli(work.path(), &["test", plugin, "cards.yaml"]);
    assert!(passed.status.success(), "{}", stdout(&passed));
    let failed = cli(work.path(), &["test", plugin, "wrong.yaml"]);
    assert!(!failed.status.success());
    assert!(
        stdout(&failed).contains("message 1 hidden"),
        "the diff names the hidden message: {}",
        stdout(&failed)
    );
}
