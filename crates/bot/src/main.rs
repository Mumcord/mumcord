//! `mumcord-bot`: drive a scripted bot from the shell (E2E tests and manual checks).
//!
//! Each run connects once, executes one subcommand (or, with `script`, one subcommand per stdin
//! line), optionally stays connected for `--hold` seconds and exits non-zero on the first error.

use std::io::BufRead;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use mumcord_bot::samples::{PNG, Sample};
use mumcord_bot::{Bot, BotConfig, MessageCheck, UserCheck, env};
use mumcord_protocol::{DenyKind, permissions};

#[derive(Parser, Debug)]
#[command(
    name = "mumcord-bot",
    about = "Scripted Mumble bot for local test servers"
)]
struct Cli {
    #[arg(long, default_value = env::HOST)]
    host: String,
    /// 64738 = v15, 64739 = v14, 64740 = v13, 64741 = limit.
    #[arg(long, default_value_t = env::PORT_V15)]
    port: u16,
    #[arg(long, default_value = "Bot")]
    name: String,
    /// Persistent certificate name (defaults to --name); reuse it to keep the identity.
    #[arg(long)]
    cert: Option<String>,
    /// Log in as SuperUser (password: SUPERUSER_PASSWORD from the environment or testenv/.env).
    #[arg(long)]
    superuser: bool,
    /// Timeout in seconds for login and for each wait.
    #[arg(long, default_value_t = 10)]
    timeout: u64,
    /// Stay connected this many seconds after the command(s).
    #[arg(long, default_value_t = 0)]
    hold: u64,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Connect and print session and certificate hash.
    Connect,
    /// Move into a channel (name or Root/Path).
    Join { channel: String },
    /// Send HTML to a channel (default: the current one).
    Say {
        #[arg(long)]
        channel: Option<String>,
        html: String,
    },
    /// Send a canned message (formatting/sanitizer tests).
    SaySample {
        #[arg(long)]
        channel: Option<String>,
        sample: Sample,
    },
    /// Send HTML privately to a user.
    Pm { user: String, html: String },
    /// Reconnect under a new name, keeping the certificate.
    Rename { new_name: String },
    /// Set the avatar from an image file (or the built-in sample with --sample).
    Avatar {
        file: Option<PathBuf>,
        #[arg(long, conflicts_with = "file")]
        sample: bool,
        /// Remove the avatar.
        #[arg(long, conflicts_with_all = ["file", "sample"])]
        clear: bool,
    },
    /// Set the comment (HTML; "" removes it).
    Comment { html: String },
    /// Self-register on the server under the current name.
    Register,
    /// Listen to (follow) channels.
    Listen { channels: Vec<String> },
    /// Stop listening to channels.
    Unlisten { channels: Vec<String> },
    /// Self mute on/off.
    Mute { on: Toggle },
    /// Self deafen on/off.
    Deaf { on: Toggle },
    /// Deny "Listen" for everyone on a channel (needs --superuser).
    DenyListen { channel: String },
    /// Undo deny-listen (needs --superuser).
    AllowListen { channel: String },
    /// Wait for a condition; exits non-zero on timeout.
    #[command(subcommand)]
    Expect(Expect),
    /// Wait a number of seconds (pacing in scripts).
    Sleep { seconds: f64 },
    /// Run subcommands from stdin, one per line (shell-like quoting, # comments), on one
    /// connection; prints "ok <line>" after each.
    Script,
}

#[derive(Subcommand, Debug)]
enum Expect {
    /// Another user's state.
    User {
        name: String,
        #[command(flatten)]
        check: UserCheck,
        /// Avatar must equal this file's bytes.
        #[arg(long)]
        avatar_file: Option<PathBuf>,
    },
    /// No user with this name is connected.
    UserGone { name: String },
    /// A received text message (since this run connected).
    Message {
        #[command(flatten)]
        check: MessageCheck,
    },
    /// A request of this bot was denied (since this run connected).
    Denied {
        /// Missing permission, e.g. listen, enter, speak, text, self-register.
        #[arg(long)]
        permission: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum Toggle {
    On,
    Off,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut bot = Bot::connect(BotConfig {
        host: cli.host.clone(),
        port: cli.port,
        name: cli.name.clone(),
        cert_name: cli.cert.clone(),
        data_dir: None,
        superuser: cli.superuser,
        password: None,
        timeout: Duration::from_secs(cli.timeout),
    })
    .await?;
    match cli.command {
        Command::Script => run_script(&mut bot).await?,
        command => run(&mut bot, command).await?,
    }
    tokio::time::sleep(Duration::from_secs(cli.hold)).await;
    Ok(())
}

async fn run_script(bot: &mut Bot) -> Result<()> {
    for (number, line) in std::io::stdin().lock().lines().enumerate() {
        let line = line.context("reading stdin")?;
        let words = split_words(&line).with_context(|| format!("line {}", number + 1))?;
        if words.is_empty() || words[0].starts_with('#') {
            continue;
        }
        let parsed = ScriptLine::try_parse_from(std::iter::once("script".to_owned()).chain(words))
            .with_context(|| format!("line {}: {line}", number + 1))?;
        if matches!(parsed.command, Command::Script) {
            bail!("line {}: script cannot be nested", number + 1);
        }
        run(bot, parsed.command)
            .await
            .with_context(|| format!("line {}: {line}", number + 1))?;
        println!("ok {line}");
    }
    Ok(())
}

#[derive(Parser, Debug)]
struct ScriptLine {
    #[command(subcommand)]
    command: Command,
}

async fn run(bot: &mut Bot, command: Command) -> Result<()> {
    match command {
        Command::Connect => print_identity(bot)?,
        Command::Join { channel } => bot.join(&channel).await?,
        Command::Say { channel, html } => bot.send_channel(channel.as_deref(), &html)?,
        Command::SaySample { channel, sample } => {
            bot.send_channel(channel.as_deref(), &sample.html())?;
        }
        Command::Pm { user, html } => bot.send_private(&user, &html)?,
        Command::Rename { new_name } => {
            bot.reconnect_as(&new_name).await?;
            print_identity(bot)?;
        }
        Command::Avatar {
            file,
            sample,
            clear,
        } => {
            let image = match (file, sample, clear) {
                (Some(path), _, _) => {
                    std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?
                }
                (None, true, _) => PNG.to_vec(),
                (None, false, true) => Vec::new(),
                (None, false, false) => bail!("give an image file, --sample or --clear"),
            };
            bot.set_avatar(image)?;
        }
        Command::Comment { html } => bot.set_comment(&html)?,
        Command::Register => bot.register().await?,
        Command::Listen { channels } => bot.listen(&as_strs(&channels))?,
        Command::Unlisten { channels } => bot.unlisten(&as_strs(&channels))?,
        Command::Mute { on } => bot.set_mute(matches!(on, Toggle::On))?,
        Command::Deaf { on } => bot.set_deaf(matches!(on, Toggle::On))?,
        Command::DenyListen { channel } => bot.deny_listen(&channel).await?,
        Command::AllowListen { channel } => bot.allow_listen(&channel).await?,
        Command::Expect(expect) => run_expect(bot, expect).await?,
        Command::Sleep { seconds } => {
            tokio::time::sleep(Duration::try_from_secs_f64(seconds).context("bad duration")?).await;
        }
        Command::Script => bail!("script is only valid as the top-level command"),
    }
    Ok(())
}

async fn run_expect(bot: &Bot, expect: Expect) -> Result<()> {
    match expect {
        Expect::User {
            name,
            mut check,
            avatar_file,
        } => {
            if let Some(path) = avatar_file {
                check.avatar = Some(
                    std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?,
                );
            }
            bot.wait_user(&name, &check).await?;
        }
        Expect::UserGone { name } => bot.wait_user_gone(&name).await?,
        Expect::Message { check } => {
            let msg = bot.wait_message(&check).await?;
            println!("{}", msg.message);
        }
        Expect::Denied { permission } => {
            let wanted = permission.as_deref().map(permission_bit).transpose()?;
            let what = permission.unwrap_or_else(|| "any".to_owned());
            let denial = bot
                .wait_denied(&what, |d| {
                    wanted.is_none_or(|bit| {
                        d.kind == DenyKind::Permission && d.permission == Some(bit)
                    })
                })
                .await?;
            println!("{:?}", denial.kind);
        }
    }
    Ok(())
}

fn print_identity(bot: &Bot) -> Result<()> {
    let user = bot.own_user()?;
    println!(
        "session={} name={} cert_hash={}",
        user.session,
        user.name,
        user.cert_hash.as_deref().unwrap_or("-")
    );
    Ok(())
}

fn permission_bit(name: &str) -> Result<u32> {
    Ok(match name {
        "write" => permissions::WRITE,
        "traverse" => permissions::TRAVERSE,
        "enter" => permissions::ENTER,
        "speak" => permissions::SPEAK,
        "whisper" => permissions::WHISPER,
        "text" => permissions::TEXT_MESSAGE,
        "listen" => permissions::LISTEN,
        "register" => permissions::REGISTER,
        "self-register" => permissions::SELF_REGISTER,
        other => bail!("unknown permission {other:?}"),
    })
}

fn as_strs(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

/// Splits a script line like a shell: whitespace separates, '…' and "…" quote, \ escapes.
fn split_words(line: &str) -> Result<Vec<String>> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut quote = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"') | None, '\\') => {
                let escaped = chars.next().context("dangling backslash")?;
                word.get_or_insert_default().push(escaped);
            }
            (Some(_), c) => word.get_or_insert_default().push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                word.get_or_insert_default();
            }
            (None, c) if c.is_whitespace() => words.extend(word.take()),
            (None, c) => word.get_or_insert_default().push(c),
        }
    }
    if quote.is_some() {
        bail!("unterminated quote");
    }
    words.extend(word);
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_like_a_shell() {
        let words =
            split_words(r#"say --channel Raid "<b>hi</b> there" 'a "b"' x\ y """#).expect("split");
        assert_eq!(
            words,
            [
                "say",
                "--channel",
                "Raid",
                "<b>hi</b> there",
                "a \"b\"",
                "x y",
                ""
            ]
        );
        assert!(split_words("say \"open").is_err());
    }

    #[test]
    fn script_lines_parse_as_subcommands() {
        let parse = |line: &str| {
            ScriptLine::try_parse_from(
                std::iter::once("script".to_owned()).chain(split_words(line).expect("split")),
            )
        };
        assert!(matches!(
            parse("expect user Alice --in Raid --muted true").map(|l| l.command),
            Ok(Command::Expect(Expect::User { .. }))
        ));
        assert!(parse("say-sample rich").is_ok());
        assert!(parse("nonsense").is_err());
    }

    #[test]
    fn cli_definition_is_valid() {
        <Cli as clap::CommandFactory>::command().debug_assert();
    }
}
