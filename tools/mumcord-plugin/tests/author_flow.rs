//! AC-6.28 author flow with the real CLI and the local SDK: `new`, `build --reproducible` twice
//! from two folders (identical hash), `check`, `test` and `preview` with the example scenario,
//! `keygen`, `sign`, `check --key`. Compiles a plugin
//! for wasm32-unknown-unknown, so it is slow and ignored by default:
//! `cargo test -p mumcord-plugin --test author_flow -- --ignored`.

use std::path::Path;
use std::process::Command;

fn cli(dir: &Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_mumcord-plugin"))
        .current_dir(dir)
        .args(args)
        // The plugin project gets its own target folder; an inherited one would mix builds.
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("the CLI runs");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success(),
        "mumcord-plugin {args:?} failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
}

fn sha256(output: &str) -> String {
    output
        .lines()
        .find_map(|l| l.strip_prefix("sha256 "))
        .expect("build prints the hash")
        .to_owned()
}

fn copy_project(from: &Path, to: &Path) {
    for entry in walk(from) {
        let relative = entry.strip_prefix(from).expect("inside");
        if relative.starts_with("target") || relative.starts_with("dist") {
            continue;
        }
        let target = to.join(relative);
        std::fs::create_dir_all(target.parent().expect("parent")).expect("test step");
        std::fs::copy(&entry, target).expect("test step");
    }
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).expect("test step") {
        let path = entry.expect("test step").path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
}

#[test]
#[ignore = "builds a wasm plugin (slow); run with --ignored"]
fn new_build_reproducibly_check_and_sign() {
    let work = tempfile::tempdir().expect("test step");
    let sdk = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/plugin-sdk");
    let sdk = sdk.to_str().expect("utf-8");
    cli(work.path(), &["new", "wave-demo", "--sdk-path", sdk]);
    let project = work.path().join("wave-demo");
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(&project)
        .args(["generate-lockfile", "--quiet"])
        .status()
        .expect("cargo runs");
    assert!(status.success());

    let first = sha256(&cli(&project, &["build", "--reproducible"]));
    let elsewhere = work.path().join("elsewhere");
    copy_project(&project, &elsewhere);
    let second = sha256(&cli(&elsewhere, &["build", "--reproducible"]));
    assert_eq!(
        first, second,
        "same source in another folder gives the same package"
    );

    let package = project.join("dist/wave-demo-0.1.0.mcplugin");
    let package = package.to_str().expect("utf-8");
    assert!(cli(work.path(), &["check", package]).contains(&first));

    // The template is the SDK example `hello`, so this also runs that example in the runtime.
    let scenario = Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios/hello.yaml");
    let scenario = scenario.to_str().expect("utf-8");
    assert!(cli(work.path(), &["test", package, scenario]).contains("ok   "));
    cli(
        work.path(),
        &[
            "preview",
            package,
            "--scenario",
            scenario,
            "--out",
            "preview.html",
        ],
    );
    let page = std::fs::read_to_string(work.path().join("preview.html")).expect("test step");
    assert!(page.contains("Waves: 2"));

    cli(
        work.path(),
        &["keygen", "--name", "author", "--unencrypted"],
    );
    cli(work.path(), &["sign", package, "--key", "author.key"]);
    cli(work.path(), &["check", package, "--key", "author.pub"]);
}
