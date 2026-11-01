//! `mumcord-plugin`: create, build, check and sign Mumcord plugins, serve a local test registry and
//! prepare registry submissions (spec 06, FR-6.71, FR-6.80).

mod build;
mod check;
mod date;
mod new;
mod publish;
mod registry;
mod serve;
mod sign;
mod site;
mod source;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "mumcord-plugin",
    version,
    about = "Tools for Mumcord plugin authors and registry maintainers"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new plugin project from the built-in template.
    New {
        /// Plugin id, e.g. `my-plugin`.
        id: String,
        /// Folder to create the project in (default: `./<id>`).
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Use a local checkout of `mumcord-plugin-sdk` instead of the git dependency.
        #[arg(long)]
        sdk_path: Option<PathBuf>,
    },
    /// Build the plugin in a project folder into a `.mcplugin` package.
    Build {
        /// Plugin project folder.
        #[arg(default_value = ".")]
        dir: PathBuf,
        /// Build so that the same source always gives the same package bytes.
        #[arg(long)]
        reproducible: bool,
        /// Output folder (default: `<dir>/dist`).
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Check a package the way Mumcord checks it before installing.
    Check {
        package: PathBuf,
        /// Apply the rules for official plugins.
        #[arg(long)]
        official: bool,
        /// Also check the author signature (`<package>.minisig`) against this public key.
        #[arg(long)]
        key: Option<String>,
    },
    /// Create a minisign key pair (`<name>.key` and `<name>.pub`).
    Keygen {
        #[arg(long, default_value = ".")]
        out: PathBuf,
        #[arg(long, default_value = "mumcord-plugin")]
        name: String,
        /// Do not protect the secret key with a password (test keys only).
        #[arg(long)]
        unencrypted: bool,
    },
    /// Sign a file; writes `<file>.minisig`.
    Sign {
        file: PathBuf,
        /// Secret key file.
        #[arg(long)]
        key: PathBuf,
    },
    /// Serve a local fixture registry signed with a test key (development builds only).
    ServeRegistry(serve::Args),
    /// Add a built package to a checkout of the plugins registry and optionally open a pull request.
    PublishPr(publish::Args),
    /// Generate the registry website data (`v1/index.json`, `revoked.json`, `permissions.json`,
    /// media) from a checkout of the plugins registry.
    SiteData(site::Args),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::New { id, dir, sdk_path } => new::run(&id, dir, sdk_path.as_deref()),
        Command::Build {
            dir,
            reproducible,
            out,
        } => build::run(&dir, reproducible, out).map(|built| {
            println!("{}", built.path.display());
            println!("sha256 {}", built.sha256);
        }),
        Command::Check {
            package,
            official,
            key,
        } => check::run(&package, official, key.as_deref()),
        Command::Keygen {
            out,
            name,
            unencrypted,
        } => sign::keygen(&out, &name, unencrypted),
        Command::Sign { file, key } => sign::sign_file(&file, &key),
        Command::ServeRegistry(args) => serve::run(args),
        Command::PublishPr(args) => publish::run(args),
        Command::SiteData(args) => site::run(args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}
