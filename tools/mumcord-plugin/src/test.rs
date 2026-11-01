//! `mumcord-plugin test`: runs a plugin in the same runtime as the client against a mock host
//! and checks YAML scenarios (FR-6.71, AC-6.28). A scenario feeds events in and expects
//! messages, UI trees, chat lines, notifications and log lines out. The mock host keeps
//! everything in a temporary folder: storage is a fresh database per scenario and nothing
//! reaches the network or the user's keychain.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use mumcord_plugins::api::storage::Storage;
use mumcord_plugins::chat::{ChatRef, GestureGate, MessageClaims, SendError, line_hash};
use mumcord_plugins::manifest::Source;
use mumcord_plugins::package::{MANIFEST_FILE, Package};
use mumcord_plugins::peer_key::PeerKeys;
use mumcord_plugins::wit::events::{
    ActionSource, ActionValue, Channel, ChatMatch, ChatTrigger, Event, PanelVisibility,
    PeerMessage, PluginSettings, SelfState, UiAction, Visibility,
};
use mumcord_plugins::wit::peers::Target;
use mumcord_plugins::wit::theme::Mode;
use mumcord_plugins::wit::types::{BotInfo, Error, Peer};
use mumcord_plugins::wit::ui::{Component, Node, Slot, UserMenuFilter};
use mumcord_plugins::{HostContext, HostEvent, Manifest, PluginHost, PluginIo};
use mumcord_store::{NewIdentity, PluginRecord, PluginVisibility, Store};
use serde::Deserialize;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

/// How long an `expect` waits for its output.
const EXPECT_TIMEOUT: Duration = Duration::from_secs(1);
/// How long `expect: nothing` watches for unexpected output.
const QUIET_TIME: Duration = Duration::from_millis(300);
const POLL: Duration = Duration::from_millis(5);
/// Largest payload of one plugin message (FR-6.34).
const MAX_PAYLOAD: usize = 997;
/// The server the mock host issues message tokens for.
const MOCK_SERVER: mumcord_store::ServerId = 1;

#[derive(clap::Args)]
pub struct Args {
    /// A built `.mcplugin` package or an unpacked plugin folder.
    pub plugin: PathBuf,
    /// Scenario files (YAML); see `tools/mumcord-plugin/scenarios/`. Chat events take
    /// `message: N` and `replay: true`; `expect: {replaced: {message: N, card: ID}}` and
    /// `expect: {hidden: {message: N}}` check `chat.replace` / `chat.hide`.
    #[arg(required = true)]
    pub scenarios: Vec<PathBuf>,
}

pub fn run(args: Args) -> Result<()> {
    let plugin = PluginFiles::open(&args.plugin)?;
    let mut failed = 0;
    for path in &args.scenarios {
        let scenario = Scenario::read(path)?;
        let name = scenario
            .name
            .clone()
            .unwrap_or_else(|| path.display().to_string());
        match Session::start(&plugin, &scenario).and_then(|mut s| s.run_steps(&scenario.steps)) {
            Ok(()) => println!("ok   {name}"),
            Err(e) => {
                failed += 1;
                println!("FAIL {name}\n     {e:#}");
            }
        }
    }
    if failed > 0 {
        bail!("{failed} of {} scenarios failed", args.scenarios.len());
    }
    Ok(())
}

/// The plugin as an unpacked folder, the way the client loads it after install.
pub struct PluginFiles {
    _unpacked: Option<tempfile::TempDir>,
    pub dir: PathBuf,
    pub manifest: Manifest,
}

impl PluginFiles {
    pub fn open(path: &Path) -> Result<Self> {
        if path.is_dir() {
            let bytes = std::fs::read(path.join(MANIFEST_FILE))
                .with_context(|| format!("{} has no {MANIFEST_FILE}", path.display()))?;
            let manifest = Manifest::parse(&bytes).context("the manifest is invalid")?;
            return Ok(Self {
                _unpacked: None,
                dir: path.to_owned(),
                manifest,
            });
        }
        let bytes =
            std::fs::read(path).with_context(|| format!("could not read {}", path.display()))?;
        // Third-party rules first; an official plugin may also use the official-only permissions.
        let package = Package::read(&bytes, Source::Repo { repository_id: 0 })
            .or_else(|third_party| Package::read(&bytes, Source::Official).map_err(|_| third_party))
            .context("invalid package")?;
        let unpacked = tempfile::tempdir()?;
        let dir = package
            .install(unpacked.path())
            .context("could not unpack the package")?;
        Ok(Self {
            _unpacked: Some(unpacked),
            dir,
            manifest: package.manifest,
        })
    }

    /// A file of the plugin, e.g. an image asset. Paths outside the plugin folder are refused.
    pub fn asset(&self, name: &str) -> Option<Vec<u8>> {
        let path = Path::new(name);
        let inside = path
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)));
        inside
            .then(|| std::fs::read(self.dir.join(path)).ok())
            .flatten()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Scenario {
    pub name: Option<String>,
    /// Granted permissions; default: the required ones plus the suggested optional ones.
    pub grant: Option<Vec<String>>,
    #[serde(default = "default_me")]
    pub me: PeerSpec,
    /// Other users in the channel who run the same plugin.
    #[serde(default)]
    pub peers: Vec<PeerSpec>,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub theme: ThemeSpec,
    #[serde(default)]
    pub steps: Vec<Step>,
}

impl Scenario {
    pub fn read(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("could not read {}", path.display()))?;
        serde_saphyr::from_str(&text).with_context(|| format!("{} is invalid", path.display()))
    }
}

impl Default for Scenario {
    fn default() -> Self {
        Self {
            name: None,
            grant: None,
            me: default_me(),
            peers: Vec::new(),
            language: default_language(),
            theme: ThemeSpec::default(),
            steps: Vec::new(),
        }
    }
}

fn default_me() -> PeerSpec {
    PeerSpec {
        session: 1,
        name: "Me".into(),
        anonymous: false,
        bot: None,
    }
}

fn default_language() -> String {
    "en".into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerSpec {
    pub session: u32,
    pub name: String,
    /// A user without a certificate: the plugin sees no key.
    #[serde(default)]
    pub anonymous: bool,
    /// What the user's hello says when they are a bot (`peers.bot`, user-menu filters).
    #[serde(default)]
    pub bot: Option<BotSpec>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BotSpec {
    pub name: String,
    #[serde(default)]
    pub modules: Vec<String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeSpec {
    #[default]
    Light,
    Dark,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub enum Step {
    Event(EventSpec),
    /// Milliseconds, e.g. to let a timer fire.
    Wait(u64),
    /// The user clicks a host-drawn `copy-button` in the slot's current tree; the mock records
    /// the text the host would put on the clipboard (see `expect: copied`).
    Copy(CopySpec),
    Expect(Expect),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CopySpec {
    /// The button's label.
    pub label: String,
    /// Defaults to the panel.
    #[serde(default)]
    pub slot: Option<SlotSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SlotSpec {
    Panel,
    MemberBadge(u32),
    ProfileCard(u32),
    ChatCard(String),
    ChatDecoration(u32),
    Settings,
    Dialog,
}

impl SlotSpec {
    pub fn to_wit(&self) -> Slot {
        match self {
            Self::Panel => Slot::Panel,
            Self::MemberBadge(s) => Slot::MemberBadge(*s),
            Self::ProfileCard(s) => Slot::ProfileCard(*s),
            Self::ChatCard(id) => Slot::ChatCard(id.clone()),
            Self::ChatDecoration(s) => Slot::ChatDecoration(*s),
            Self::Settings => Slot::Settings,
            Self::Dialog => Slot::Dialog,
        }
    }

    pub fn from_wit(slot: &Slot) -> Self {
        match slot {
            Slot::Panel => Self::Panel,
            Slot::MemberBadge(s) => Self::MemberBadge(*s),
            Slot::ProfileCard(s) => Self::ProfileCard(*s),
            Slot::ChatCard(id) => Self::ChatCard(id.clone()),
            Slot::ChatDecoration(s) => Self::ChatDecoration(*s),
            Slot::Settings => Self::Settings,
            Slot::Dialog => Self::Dialog,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub enum EventSpec {
    /// A click, toggle, input … in a slot (a user gesture), or a typed slash command.
    UiAction {
        action: String,
        #[serde(default)]
        slot: Option<SlotSpec>,
        #[serde(default)]
        command: bool,
        #[serde(default)]
        value: Option<ValueSpec>,
    },
    PeerMessage {
        from: u32,
        kind: u8,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        hex: Option<String>,
        #[serde(default)]
        direct: bool,
    },
    Joined(u32),
    Left(u32),
    Talking {
        session: u32,
        talking: bool,
    },
    ScreenShareStarted(u32),
    ChatActivity(u32),
    /// A chat line starting with one of the plugin's trigger words, e.g. `!poll 2`.
    ChatTrigger {
        sender: u32,
        trigger: String,
        #[serde(default)]
        args: String,
        /// The chat message this line is, as `expect: replaced` / `hidden` name it. Defaults to
        /// the next unused number (1, 2, …); the same number twice is the same message.
        #[serde(default)]
        message: Option<u64>,
        /// The line comes from stored history (`chat.is-replay` is true). Like the real host,
        /// the mock delivers it only to plugins that import `chat.is-replay`.
        #[serde(default)]
        replay: bool,
    },
    /// A channel message matching one of the plugin's chat patterns, e.g. a pasted fit. The
    /// pattern index is the first manifest pattern that matches `text`.
    ChatMatch {
        sender: u32,
        text: String,
        /// See `chat-trigger`.
        #[serde(default)]
        message: Option<u64>,
        /// See `chat-trigger`.
        #[serde(default)]
        replay: bool,
    },
    Mentioned(u32),
    ProfileCardOpened(u32),
    UserMenuRequested(u32),
    PanelVisibility(PanelSpec),
    DialogClosed,
    Language(String),
    /// The user's own state changed (mute, deafen, do not disturb, channel).
    /// Unset fields keep their defaults: not muted, not deafened, DND off, test channel.
    SelfChanged {
        #[serde(default)]
        muted: bool,
        #[serde(default)]
        deafened: bool,
        #[serde(default)]
        dnd: bool,
        /// Name of the channel the user is in.
        #[serde(default)]
        channel: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ValueSpec {
    Boolean(bool),
    Number(f32),
    Text(String),
    Reorder { from: u32, to: u32 },
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PanelSpec {
    Hidden,
    Inline,
    Expanded,
}

/// Text fields match as substrings, `message` payloads exactly.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub enum Expect {
    Tree {
        slot: SlotSpec,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        nodes: Option<usize>,
    },
    Cleared(SlotSpec),
    Message {
        /// `channel` or a session number.
        #[serde(default)]
        to: Option<ToSpec>,
        #[serde(default)]
        kind: Option<u8>,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        hex: Option<String>,
    },
    Chat {
        #[serde(default)]
        text: Option<String>,
    },
    Notification {
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        body: Option<String>,
    },
    Dialog {
        #[serde(default)]
        title: Option<String>,
    },
    /// The plugin opened (`open: true`, the default) or closed its panel popover.
    PanelPopover {
        #[serde(default = "default_true")]
        open: bool,
    },
    MenuEntry {
        #[serde(default)]
        label: Option<String>,
        /// The users a filtered entry shows on; checked only when given.
        #[serde(default)]
        sessions: Option<Vec<u32>>,
    },
    Log {
        contains: String,
    },
    /// The plugin failed and was stopped.
    Stopped {
        #[serde(default)]
        error: Option<String>,
    },
    /// The plugin put chat card `card` (any card when omitted) in the place of `message`
    /// (`chat.replace`).
    Replaced {
        message: u64,
        #[serde(default)]
        card: Option<String>,
    },
    /// The plugin hid `message` (`chat.hide`).
    Hidden {
        message: u64,
    },
    /// The host copied exactly this text after a `copy` step.
    Copied {
        text: String,
    },
    /// No output at all for a moment.
    Nothing,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ToSpec {
    Session(u32),
    Word(String),
}

/// Something the plugin did that a scenario can expect.
#[derive(Debug, Clone)]
pub enum Output {
    Tree {
        slot: SlotSpec,
        nodes: Vec<Node>,
    },
    Cleared(SlotSpec),
    /// Title and subtitle.
    Dialog(String, Option<String>),
    DialogClosed,
    /// The panel popover opened (`true`) or closed (`false`).
    PanelPopover(bool),
    /// Label (`None`: removed) and, for a filtered entry, the matching sessions.
    MenuEntry(Option<String>, Option<Vec<u32>>),
    Message {
        to: Target,
        kind: u8,
        payload: Vec<u8>,
    },
    Chat(String),
    Supersede {
        card: String,
        sender: u32,
    },
    /// `chat.replace` (`Some` card) or `chat.hide` (`None`) of a scenario message number.
    Replaced {
        message: u64,
        card: Option<String>,
    },
    Notification {
        title: String,
        body: String,
    },
    Stopped(String),
    /// The text a clicked `copy-button` put on the (mock) clipboard.
    Copied(String),
}

/// The mock side of [`PluginIo`]: records every call and plays the channel the scenario
/// describes.
#[derive(Debug)]
struct MockIo {
    name: String,
    max_payload: usize,
    channel: Vec<Peer>,
    /// Bot info by session, for the users in `channel`.
    bots: HashMap<u32, BotInfo>,
    outputs: Arc<Mutex<Vec<Output>>>,
    gestures: Mutex<GestureGate>,
    /// The message tokens handed out with chat events, like the real host's.
    claims: Mutex<MessageClaims>,
}

impl MockIo {
    fn push(&self, output: Output) {
        lock(&self.outputs).push(output);
    }
}

impl PluginIo for MockIo {
    fn set_tree(&self, _plugin: &str, slot: Slot, nodes: Vec<Node>) -> Result<(), Error> {
        let slot = SlotSpec::from_wit(&slot);
        self.push(Output::Tree { slot, nodes });
        Ok(())
    }

    fn clear(&self, _plugin: &str, slot: Slot) {
        self.push(Output::Cleared(SlotSpec::from_wit(&slot)));
    }

    fn open_dialog(
        &self,
        _plugin: &str,
        title: String,
        subtitle: Option<String>,
    ) -> Result<(), Error> {
        self.push(Output::Dialog(title, subtitle));
        Ok(())
    }

    fn close_dialog(&self, _plugin: &str) {
        self.push(Output::DialogClosed);
    }

    fn open_panel_popover(&self, plugin: &str) -> Result<(), Error> {
        if !lock(&self.gestures).is_fresh(plugin, Instant::now()) {
            return Err(Error::NotPermitted);
        }
        self.push(Output::PanelPopover(true));
        Ok(())
    }

    fn close_panel_popover(&self, _plugin: &str) {
        self.push(Output::PanelPopover(false));
    }

    fn set_user_menu_entry(&self, _plugin: &str, label: Option<String>) -> Result<(), Error> {
        self.push(Output::MenuEntry(label, None));
        Ok(())
    }

    /// Resolves the filter once against the scenario's channel, as the client does.
    fn set_user_menu_entry_for(
        &self,
        _plugin: &str,
        label: String,
        filter: UserMenuFilter,
    ) -> Result<(), Error> {
        let wants_bot = filter.bot_name.is_some() || filter.bot_module.is_some();
        let sessions = self.channel[1..]
            .iter()
            .map(|peer| peer.session)
            .filter(|session| {
                let bot_matches = self.bots.get(session).is_some_and(|bot| {
                    filter.bot_name.as_ref().is_none_or(|n| *n == bot.name)
                        && filter
                            .bot_module
                            .as_ref()
                            .is_none_or(|m| bot.modules.contains(m))
                });
                (wants_bot && bot_matches) || filter.sessions.contains(session)
            })
            .collect();
        self.push(Output::MenuEntry(Some(label), Some(sessions)));
        Ok(())
    }

    fn peer_send(
        &self,
        _plugin: &str,
        to: Target,
        kind: u8,
        payload: Vec<u8>,
    ) -> Result<(), Error> {
        if payload.len() > self.max_payload {
            return Err(Error::TooLarge);
        }
        let others = &self.channel[1..];
        let reachable = match to {
            Target::Channel => !others.is_empty(),
            Target::Direct(session) => others.iter().any(|p| p.session == session),
        };
        if !reachable {
            return Err(Error::NoRecipients);
        }
        self.push(Output::Message { to, kind, payload });
        Ok(())
    }

    fn start_game_burst(&self, _plugin: &str) -> Result<(), Error> {
        Ok(())
    }

    fn presence(&self, _plugin: &str) -> Result<Vec<Peer>, Error> {
        Ok(self.channel.clone())
    }

    fn bot(&self, _plugin: &str, session: u32) -> Result<Option<BotInfo>, Error> {
        Ok(self.bots.get(&session).cloned())
    }

    fn chat_send(&self, plugin: &str, text: String) -> Result<(), Error> {
        let line = lock(&self.gestures)
            .send(plugin, &self.name, &text, Instant::now())
            .map_err(|e| match e {
                SendError::NotPermitted => Error::NotPermitted,
                SendError::TooLarge => Error::TooLarge,
                SendError::Invalid => Error::Invalid,
            })?;
        self.push(Output::Chat(line));
        Ok(())
    }

    fn line_hash(&self, _plugin: &str, text: &str) -> u32 {
        line_hash(&self.name, text)
    }

    fn supersede(&self, _plugin: &str, card: String, sender: u32, _hash: u32) -> Result<(), Error> {
        self.push(Output::Supersede { card, sender });
        Ok(())
    }

    fn replace_message(&self, plugin: &str, token: u64, card: Option<String>) -> Result<(), Error> {
        let claim = lock(&self.claims)
            .claim(plugin, token)
            .ok_or(Error::NotPermitted)?;
        self.push(Output::Replaced {
            message: claim.message,
            card,
        });
        Ok(())
    }
}

/// One running plugin in the mock host.
pub struct Session {
    _data: tempfile::TempDir,
    id: String,
    host: PluginHost,
    events: UnboundedReceiver<HostEvent>,
    io: Arc<MockIo>,
    me: Peer,
    outputs: Arc<Mutex<Vec<Output>>>,
    /// Outputs before this index were matched or skipped by an earlier `expect`.
    cursor: usize,
    /// The manifest's trigger words; a `chat-trigger` step must use one of them.
    triggers: Vec<String>,
    /// The manifest's chat patterns; a `chat-match` step's text must match one of them.
    chat_patterns: Vec<String>,
    /// The highest scenario message number used so far; `chat-*` steps without a `message` get
    /// the next one.
    last_message: u64,
}

impl Session {
    /// Loads and enables the plugin, then sends what the client sends after a start.
    pub fn start(plugin: &PluginFiles, scenario: &Scenario) -> Result<Self> {
        let manifest = &plugin.manifest;
        let id = manifest.id.clone();
        let data = tempfile::tempdir()?;
        let store = Store::open(data.path()).context("could not create the test database")?;
        store.save_plugin(&record(manifest))?;
        let identity = store.add_identity(&NewIdentity::new("Test", "test", "00", 0))?;
        let storage = Storage::new(Arc::new(Mutex::new(store)));
        storage.set_identity(Some(identity.id));
        let keys = PeerKeys::load_or_create(data.path())?;

        let peer = |spec: &PeerSpec| Peer {
            session: spec.session,
            name: spec.name.clone(),
            key: (!spec.anonymous)
                .then(|| keys.key(&id, Some(&format!("test-cert-{}", spec.session))))
                .flatten(),
        };
        let me = peer(&scenario.me);
        let mut channel = vec![me.clone()];
        channel.extend(scenario.peers.iter().map(peer));
        let bots = scenario
            .peers
            .iter()
            .filter_map(|spec| {
                let bot = spec.bot.as_ref()?;
                Some((
                    spec.session,
                    BotInfo {
                        name: bot.name.clone(),
                        modules: bot.modules.clone(),
                    },
                ))
            })
            .collect();

        let outputs = Arc::new(Mutex::new(Vec::new()));
        let io = Arc::new(MockIo {
            name: manifest.name.clone(),
            max_payload: (manifest.messaging.max_payload as usize).min(MAX_PAYLOAD),
            channel,
            bots,
            outputs: Arc::clone(&outputs),
            gestures: Mutex::default(),
            claims: Mutex::default(),
        });
        let ctx = Arc::new(HostContext {
            storage,
            secrets: None,
            peer_keys: Some(keys),
            io: Some(Arc::clone(&io) as Arc<dyn PluginIo>),
            ..HostContext::default()
        });
        let granted = scenario.grant.clone().unwrap_or_else(|| {
            let permissions = &manifest.permissions;
            let suggested = permissions.optional.iter().filter(|p| p.suggested);
            let mut granted = permissions.required.clone();
            granted.extend(suggested.map(|p| p.id.clone()));
            granted
        });
        ctx.grants.set(&id, granted.iter().cloned());
        let mode = match scenario.theme {
            ThemeSpec::Light => Mode::Light,
            ThemeSpec::Dark => Mode::Dark,
        };
        ctx.theme.set(mode, false);

        let (outbox, events) = unbounded_channel();
        let version = semver::Version::parse(env!("CARGO_PKG_VERSION"))?;
        let mut host = PluginHost::new(version, None, Arc::clone(&ctx), outbox)
            .context("the plugin runtime did not start")?;
        host.load(&plugin.dir, None)
            .context("the plugin was refused")?;
        let mut session = Self {
            _data: data,
            id,
            host,
            events,
            io,
            me: me.clone(),
            outputs,
            cursor: 0,
            triggers: manifest.triggers.clone(),
            chat_patterns: manifest
                .chat_patterns
                .iter()
                .map(|p| p.pattern.clone())
                .collect(),
            last_message: 0,
        };
        if let Err(e) = session.host.enable(&session.id) {
            session.drain_host_events();
            let logs = session.log_text();
            bail!("the plugin did not start: {e}{logs}");
        }
        for event in [
            Event::SettingsChanged(PluginSettings {
                granted,
                visibility: Visibility::Everyone,
            }),
            Event::LanguageChanged(scenario.language.clone()),
            Event::ThemeChanged(ctx.theme.info()),
            Event::SelfChanged(SelfState {
                me: Some(me),
                server: Some("Test server".into()),
                channel: Some(test_channel()),
                muted: false,
                deafened: false,
                do_not_disturb: false,
            }),
        ] {
            session.host.send(&session.id, event);
        }
        Ok(session)
    }

    pub fn run_steps(&mut self, steps: &[Step]) -> Result<()> {
        for (n, step) in steps.iter().enumerate() {
            self.step(step)
                .with_context(|| format!("step {} ({})", n + 1, describe(step)))?;
        }
        Ok(())
    }

    /// Runs only the events and waits of a scenario (for `preview`).
    pub fn play(&mut self, steps: &[Step]) -> Result<()> {
        for step in steps {
            if !matches!(step, Step::Expect(_)) {
                self.step(step)?;
            }
        }
        Ok(())
    }

    fn step(&mut self, step: &Step) -> Result<()> {
        match step {
            Step::Event(spec) => {
                let event = self.event(spec)?;
                let chat = match spec {
                    EventSpec::ChatMatch {
                        message, replay, ..
                    }
                    | EventSpec::ChatTrigger {
                        message, replay, ..
                    } => Some((*message, *replay)),
                    _ => None,
                };
                match chat {
                    Some((message, replay)) => self.send_chat(event, message, replay),
                    None => {
                        self.host.send(&self.id, event);
                    }
                }
                Ok(())
            }
            Step::Wait(ms) => {
                std::thread::sleep(Duration::from_millis(*ms));
                Ok(())
            }
            Step::Copy(spec) => self.copy(spec),
            Step::Expect(expect) => self.expect(expect),
        }
    }

    /// Sends a chat event with a token for its message, valid only while the plugin handles it
    /// (the real host hands the token out the same way).
    fn send_chat(&mut self, event: Event, message: Option<u64>, replay: bool) {
        let message = message.unwrap_or(self.last_message + 1);
        self.last_message = self.last_message.max(message);
        let token = lock(&self.io.claims).issue(&self.id, MOCK_SERVER, message);
        let chat = ChatRef {
            token: Some(token),
            replay,
        };
        self.host.send_chat(&self.id, event, chat);
    }

    /// Plays the host's side of a copy-button click: finds the button in the slot's latest tree
    /// and records its text. The plugin itself learns nothing about the click.
    fn copy(&mut self, spec: &CopySpec) -> Result<()> {
        let slot = spec.slot.clone().unwrap_or(SlotSpec::Panel);
        let trees = self.trees();
        let nodes = trees
            .iter()
            .find(|(s, _)| *s == slot)
            .map(|(_, nodes)| nodes)
            .with_context(|| format!("the plugin has no tree in {slot:?}"))?;
        let text = nodes
            .iter()
            .find_map(|node| match &node.component {
                Component::CopyButton(b) if b.label == spec.label => Some(b.text.clone()),
                _ => None,
            })
            .with_context(|| format!("no copy button labelled {:?} in {slot:?}", spec.label))?;
        self.io.push(Output::Copied(text));
        Ok(())
    }

    fn peer(&self, session: u32) -> Result<Peer> {
        self.io
            .channel
            .iter()
            .find(|p| p.session == session)
            .cloned()
            .with_context(|| format!("session {session} is not in the scenario"))
    }

    fn event(&self, spec: &EventSpec) -> Result<Event> {
        Ok(match spec {
            EventSpec::UiAction {
                action,
                slot,
                command,
                value,
            } => {
                let mut gestures = lock(&self.io.gestures);
                let source = if *command {
                    gestures.command_gesture(&self.id, Instant::now());
                    ActionSource::Command
                } else {
                    gestures.gesture(&self.id, Instant::now());
                    ActionSource::Slot(slot.as_ref().unwrap_or(&SlotSpec::Panel).to_wit())
                };
                let value = match value {
                    None => ActionValue::None,
                    Some(ValueSpec::Boolean(b)) => ActionValue::Boolean(*b),
                    Some(ValueSpec::Number(n)) => ActionValue::Number(*n),
                    Some(ValueSpec::Text(t)) => ActionValue::Text(t.clone()),
                    Some(ValueSpec::Reorder { from, to }) => ActionValue::Reorder((*from, *to)),
                };
                Event::UiAction(UiAction {
                    source,
                    action: action.clone(),
                    value,
                })
            }
            EventSpec::PeerMessage {
                from,
                kind,
                text,
                hex,
                direct,
            } => Event::PeerMessage(PeerMessage {
                sender: self.peer(*from)?,
                kind: *kind,
                payload: payload(text.as_deref(), hex.as_deref())?,
                direct: *direct,
            }),
            EventSpec::Joined(s) => Event::Joined(self.peer(*s)?),
            EventSpec::Left(s) => Event::Left(self.peer(*s)?),
            EventSpec::Talking { session, talking } => {
                Event::Talking((self.peer(*session)?, *talking))
            }
            EventSpec::ScreenShareStarted(s) => Event::ScreenShareStarted(self.peer(*s)?),
            EventSpec::ChatActivity(s) => Event::ChatActivity((self.peer(*s)?, test_channel())),
            EventSpec::ChatTrigger {
                sender,
                trigger,
                args,
                ..
            } => {
                if !self.triggers.contains(trigger) {
                    bail!("the manifest does not declare the trigger \"{trigger}\"");
                }
                Event::ChatTrigger(ChatTrigger {
                    sender: self.peer(*sender)?,
                    trigger: trigger.clone(),
                    args: mumcord_plugins::triggers::clip_args(args),
                })
            }
            EventSpec::ChatMatch { sender, text, .. } => {
                let Some(pattern_index) =
                    mumcord_plugins::patterns::first_match(&self.chat_patterns, text)
                else {
                    bail!("the text matches none of the manifest's chat patterns");
                };
                Event::ChatMatch(ChatMatch {
                    sender: self.peer(*sender)?,
                    pattern_index,
                    text: mumcord_plugins::patterns::clip_text(text),
                })
            }
            EventSpec::Mentioned(s) => Event::Mentioned(self.peer(*s)?),
            EventSpec::ProfileCardOpened(s) => Event::ProfileCardOpened(self.peer(*s)?),
            EventSpec::UserMenuRequested(s) => Event::UserMenuRequested(self.peer(*s)?),
            EventSpec::PanelVisibility(v) => Event::PanelVisibilityChanged(match v {
                PanelSpec::Hidden => PanelVisibility::Hidden,
                PanelSpec::Inline => PanelVisibility::Inline,
                PanelSpec::Expanded => PanelVisibility::Expanded,
            }),
            EventSpec::DialogClosed => Event::DialogClosed,
            EventSpec::Language(l) => Event::LanguageChanged(l.clone()),
            EventSpec::SelfChanged {
                muted,
                deafened,
                dnd,
                channel,
            } => {
                let mut channel_state = test_channel();
                if let Some(name) = channel {
                    channel_state.name = name.clone();
                }
                Event::SelfChanged(SelfState {
                    me: Some(self.me.clone()),
                    server: Some("Test server".into()),
                    channel: Some(channel_state),
                    muted: *muted,
                    deafened: *deafened,
                    do_not_disturb: *dnd,
                })
            }
        })
    }

    fn expect(&mut self, expect: &Expect) -> Result<()> {
        if let Expect::Nothing = expect {
            std::thread::sleep(QUIET_TIME);
            self.drain_host_events();
            let outputs = lock(&self.outputs);
            if let Some(output) = outputs.get(self.cursor) {
                bail!("expected no output, got {}", summary(output));
            }
            return Ok(());
        }
        if let Expect::Log { contains } = expect {
            let deadline = Instant::now() + EXPECT_TIMEOUT;
            loop {
                if self
                    .host
                    .log_lines(&self.id)
                    .iter()
                    .any(|(_, line)| line.contains(contains.as_str()))
                {
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    bail!("no log line contains {contains:?}{}", self.log_text());
                }
                std::thread::sleep(POLL);
            }
        }
        let deadline = Instant::now() + EXPECT_TIMEOUT;
        loop {
            self.drain_host_events();
            {
                let outputs = lock(&self.outputs);
                if let Some(offset) = outputs[self.cursor..]
                    .iter()
                    .position(|o| matches(expect, o))
                {
                    self.cursor += offset + 1;
                    return Ok(());
                }
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(POLL);
        }
        let outputs = lock(&self.outputs);
        let seen: Vec<String> = outputs[self.cursor..].iter().map(summary).collect();
        if seen.is_empty() {
            bail!("no matching output within {EXPECT_TIMEOUT:?}; the plugin produced nothing");
        }
        bail!(
            "no matching output within {EXPECT_TIMEOUT:?}; got:\n       {}",
            seen.join("\n       ")
        );
    }

    /// Moves notifications and failures from the host into the output list.
    fn drain_host_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            let output = match event {
                HostEvent::Notify { title, body, .. } => Output::Notification { title, body },
                HostEvent::Failed { error, .. } => Output::Stopped(error.to_string()),
                _ => continue,
            };
            self.io.push(output);
        }
    }

    fn log_text(&self) -> String {
        let lines = self.host.log_lines(&self.id);
        if lines.is_empty() {
            return String::new();
        }
        let lines: Vec<String> = lines
            .iter()
            .map(|(level, line)| format!("{level:?}: {line}"))
            .collect();
        format!("\n     plugin log:\n       {}", lines.join("\n       "))
    }

    /// The latest tree per slot, in the order the slots were first filled; cleared slots are
    /// left out.
    pub fn trees(&mut self) -> Vec<(SlotSpec, Vec<Node>)> {
        self.drain_host_events();
        let mut trees: Vec<(SlotSpec, Vec<Node>)> = Vec::new();
        for output in lock(&self.outputs).iter() {
            match output {
                Output::Tree { slot, nodes } => match trees.iter_mut().find(|(s, _)| s == slot) {
                    Some(entry) => entry.1 = nodes.clone(),
                    None => trees.push((slot.clone(), nodes.clone())),
                },
                Output::Cleared(slot) => trees.retain(|(s, _)| s != slot),
                _ => {}
            }
        }
        trees
    }

    /// The dialog title and subtitle, if the plugin left its dialog open.
    pub fn dialog(&self) -> Option<(String, Option<String>)> {
        let mut title = None;
        for output in lock(&self.outputs).iter() {
            match output {
                Output::Dialog(t, s) => title = Some((t.clone(), s.clone())),
                Output::DialogClosed => title = None,
                _ => {}
            }
        }
        title
    }

    /// Whether the plugin left its panel popover open.
    pub fn panel_popover_open(&self) -> bool {
        let mut open = false;
        for output in lock(&self.outputs).iter() {
            if let Output::PanelPopover(o) = output {
                open = *o;
            }
        }
        open
    }

    pub fn stop(mut self) {
        let _ = self.host.disable(&self.id);
    }
}

fn record(manifest: &Manifest) -> PluginRecord {
    PluginRecord {
        id: manifest.id.clone(),
        version: manifest.version.to_string(),
        previous_version: None,
        source: "dev".into(),
        sha256: None,
        author_key: None,
        enabled: true,
        disabled_reason: None,
        visibility: PluginVisibility::Everyone,
        granted: vec![],
        pinned: false,
        notifications: true,
        removed_at: None,
        installed_at: 0,
    }
}

fn test_channel() -> Channel {
    Channel {
        id: 0,
        name: "Root".into(),
    }
}

fn payload(text: Option<&str>, hex: Option<&str>) -> Result<Vec<u8>> {
    match (text, hex) {
        (Some(_), Some(_)) => bail!("give either text or hex, not both"),
        (Some(text), None) => Ok(text.as_bytes().to_vec()),
        (None, Some(hex)) => decode_hex(hex),
        (None, None) => Ok(Vec::new()),
    }
}

fn decode_hex(hex: &str) -> Result<Vec<u8>> {
    let hex: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
    if !hex.len().is_multiple_of(2) {
        bail!("hex {hex:?} has an odd length");
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).with_context(|| format!("bad hex {hex:?}")))
        .collect()
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn contains(haystack: &str, needle: Option<&String>) -> bool {
    needle.is_none_or(|n| haystack.contains(n.as_str()))
}

fn matches(expect: &Expect, output: &Output) -> bool {
    match (expect, output) {
        (Expect::Tree { slot, text, nodes }, Output::Tree { slot: s, nodes: n }) => {
            slot == s
                && nodes.is_none_or(|count| count == n.len())
                && text.as_ref().is_none_or(|t| {
                    n.iter()
                        .flat_map(node_texts)
                        .any(|s| s.contains(t.as_str()))
                })
        }
        (Expect::Cleared(slot), Output::Cleared(s)) => slot == s,
        (
            Expect::Message {
                to,
                kind,
                text,
                hex,
            },
            Output::Message {
                to: t,
                kind: k,
                payload,
            },
        ) => {
            let to_matches = match (to, t) {
                (None, _) => true,
                (Some(ToSpec::Word(w)), Target::Channel) => w == "channel",
                (Some(ToSpec::Session(s)), Target::Direct(d)) => s == d,
                _ => false,
            };
            to_matches
                && kind.is_none_or(|kind| kind == *k)
                && text
                    .as_ref()
                    .is_none_or(|t| t.as_bytes() == payload.as_slice())
                && hex
                    .as_ref()
                    .is_none_or(|h| decode_hex(h).is_ok_and(|h| h == *payload))
        }
        (Expect::Chat { text }, Output::Chat(line)) => contains(line, text.as_ref()),
        (Expect::Notification { title, body }, Output::Notification { title: t, body: b }) => {
            contains(t, title.as_ref()) && contains(b, body.as_ref())
        }
        (Expect::Dialog { title }, Output::Dialog(t, _)) => contains(t, title.as_ref()),
        (Expect::PanelPopover { open }, Output::PanelPopover(o)) => open == o,
        (Expect::MenuEntry { label, sessions }, Output::MenuEntry(l, s)) => {
            label == l && (sessions.is_none() || sessions == s)
        }
        (
            Expect::Replaced { message, card },
            Output::Replaced {
                message: m,
                card: Some(c),
            },
        ) => message == m && card.as_ref().is_none_or(|card| card == c),
        (
            Expect::Hidden { message },
            Output::Replaced {
                message: m,
                card: None,
            },
        ) => message == m,
        (Expect::Stopped { error }, Output::Stopped(e)) => contains(e, error.as_ref()),
        (Expect::Copied { text }, Output::Copied(t)) => text == t,
        _ => false,
    }
}

/// Every text a node shows or announces (labels, values, alt texts).
pub fn node_texts(node: &Node) -> Vec<&str> {
    match &node.component {
        Component::Stack(_) | Component::Divider => vec![],
        Component::Text(t) => vec![&t.text],
        Component::Image(i) => vec![&i.alt],
        Component::Sprite(s) => vec![&s.alt],
        Component::Meter(m) => vec![&m.label],
        Component::Button(b) => vec![&b.label],
        Component::IconButton(b) => vec![&b.label],
        Component::Chip(c) => vec![&c.text],
        Component::Progress(p) => vec![&p.label],
        Component::List(l) => vec![&l.label],
        Component::Toggle(t) => vec![&t.label],
        Component::Select(s) => std::iter::once(s.label.as_str())
            .chain(s.options.iter().map(|o| o.label.as_str()))
            .collect(),
        Component::Slider(s) => vec![&s.label],
        Component::TextInput(t) => [t.label.as_str(), t.value.as_str()]
            .into_iter()
            .chain(t.help.as_deref())
            .chain(t.error.as_deref())
            .collect(),
        Component::Password(p) => [p.label.as_str()]
            .into_iter()
            .chain(p.help.as_deref())
            .chain(p.error.as_deref())
            .collect(),
        Component::FilePicker(f) => vec![&f.label],
        Component::Countdown(c) => vec![&c.label],
        Component::CopyButton(b) => vec![&b.label],
    }
}

fn summary(output: &Output) -> String {
    match output {
        Output::Tree { slot, nodes } => {
            let texts: Vec<&str> = nodes.iter().flat_map(node_texts).collect();
            format!("tree {slot:?}, {} nodes, texts {texts:?}", nodes.len())
        }
        Output::Cleared(slot) => format!("cleared {slot:?}"),
        Output::Dialog(title, None) => format!("dialog {title:?}"),
        Output::Dialog(title, Some(subtitle)) => format!("dialog {title:?} ({subtitle:?})"),
        Output::DialogClosed => "dialog closed".into(),
        Output::PanelPopover(open) => {
            format!("panel popover {}", if *open { "opened" } else { "closed" })
        }
        Output::MenuEntry(label, None) => format!("menu entry {label:?}"),
        Output::MenuEntry(label, Some(sessions)) => {
            format!("menu entry {label:?} for {sessions:?}")
        }
        Output::Message { to, kind, payload } => format!(
            "message to {to:?}, kind {kind}, payload {}",
            encode_hex(payload)
        ),
        Output::Chat(line) => format!("chat {line:?}"),
        Output::Supersede { card, sender } => format!("supersede card {card:?} of {sender}"),
        Output::Replaced {
            message,
            card: Some(card),
        } => format!("message {message} replaced by card {card:?}"),
        Output::Replaced {
            message,
            card: None,
        } => format!("message {message} hidden"),
        Output::Notification { title, body } => format!("notification {title:?} {body:?}"),
        Output::Stopped(error) => format!("stopped: {error}"),
        Output::Copied(text) => format!("copied {text:?}"),
    }
}

fn describe(step: &Step) -> String {
    match step {
        Step::Event(e) => format!("event {e:?}"),
        Step::Wait(ms) => format!("wait {ms} ms"),
        Step::Copy(c) => format!("copy {:?}", c.label),
        Step::Expect(e) => format!("expect {e:?}"),
    }
}

fn default_true() -> bool {
    true
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn copy_steps_and_expectations_parse_and_match() {
        let yaml = r#"
steps:
  - copy: {label: Copy fitting}
  - copy: {label: Copy, slot: dialog}
  - expect: {copied: {text: "[Drake]"}}
"#;
        let scenario: Scenario = serde_saphyr::from_str(yaml).expect("parses");
        assert!(matches!(&scenario.steps[0], Step::Copy(c) if c.slot.is_none()));
        assert!(matches!(
            &scenario.steps[1],
            Step::Copy(c) if c.slot == Some(SlotSpec::Dialog)
        ));
        let Step::Expect(expect) = &scenario.steps[2] else {
            panic!("not an expectation")
        };
        assert!(matches(expect, &Output::Copied("[Drake]".into())));
        assert!(!matches(expect, &Output::Copied("[Drake] ".into())));
        assert!(!matches(expect, &Output::Chat("[Drake]".into())));
    }

    #[test]
    fn scenarios_parse_from_yaml() {
        let yaml = r#"
name: Waves
grant: [ui.panel, peers.messages]
peers:
  - {session: 2, name: Bob}
steps:
  - event: {ui-action: {action: wave}}
  - event: {ui-action: {action: pick, slot: {chat-card: c1}, value: "a"}}
  - event: {peer-message: {from: 2, kind: 1, hex: "00ff"}}
  - event: {joined: 2}
  - event: dialog-closed
  - event: {self-changed: {dnd: true, muted: true, channel: Lobby}}
  - event: {chat-trigger: {sender: 2, trigger: "!poll", args: "2"}}
  - wait: 50
  - event: {chat-match: {sender: 2, text: "[Rifter, PvP]\nDamage Control II"}}
  - expect: {tree: {slot: panel, text: "Waves: 1"}}
  - expect: {message: {to: channel, kind: 1}}
  - expect: {message: {to: 2}}
  - expect: nothing
  - event: {chat-trigger: {sender: 2, trigger: "!poll", message: 4, replay: true}}
  - expect: {replaced: {message: 4, card: c1}}
  - expect: {hidden: {message: 4}}
"#;
        let scenario: Scenario = serde_saphyr::from_str(yaml).expect("parses");
        assert_eq!(scenario.steps.len(), 16);
        assert!(matches!(
            &scenario.steps[13],
            Step::Event(EventSpec::ChatTrigger {
                message: Some(4),
                replay: true,
                ..
            })
        ));
        assert!(matches!(
            &scenario.steps[14],
            Step::Expect(Expect::Replaced { message: 4, card: Some(c) }) if c == "c1"
        ));
        assert!(matches!(
            &scenario.steps[15],
            Step::Expect(Expect::Hidden { message: 4 })
        ));
        assert_eq!(scenario.me.session, 1);
        assert!(matches!(
            &scenario.steps[1],
            Step::Event(EventSpec::UiAction { slot: Some(SlotSpec::ChatCard(c)), value: Some(ValueSpec::Text(v)), .. })
                if c == "c1" && v == "a"
        ));
        assert!(matches!(
            &scenario.steps[5],
            Step::Event(EventSpec::SelfChanged { dnd: true, muted: true, deafened: false, channel: Some(c) })
                if c == "Lobby"
        ));
        assert!(matches!(
            &scenario.steps[6],
            Step::Event(EventSpec::ChatTrigger { sender: 2, trigger, args, message: None, replay: false })
                if trigger == "!poll" && args == "2"
        ));
        assert!(matches!(
            &scenario.steps[8],
            Step::Event(EventSpec::ChatMatch { sender: 2, text, message: None, replay: false })
                if text == "[Rifter, PvP]\nDamage Control II"
        ));
        assert!(matches!(
            &scenario.steps[11],
            Step::Expect(Expect::Message {
                to: Some(ToSpec::Session(2)),
                ..
            })
        ));
    }

    #[test]
    fn node_texts_include_field_help_and_error() {
        use mumcord_plugins::wit::ui::{InputPurpose, Password, TextInput};
        let input = Node {
            component: Component::TextInput(TextInput {
                label: "Address".into(),
                value: String::new(),
                placeholder: String::new(),
                max_bytes: 64,
                purpose: InputPurpose::ServerAddress,
                action: "address".into(),
                disabled: false,
                help: Some("Reachable.".into()),
                help_tone: None,
                error: Some("Not reachable".into()),
            }),
            animation: None,
            children: vec![],
        };
        let secret = Node {
            component: Component::Password(Password {
                label: "Token".into(),
                secret: "token".into(),
                action: "save".into(),
                disabled: false,
                help: Some("Stored safely.".into()),
                help_tone: None,
                error: Some("Too short".into()),
            }),
            animation: None,
            children: vec![],
        };
        assert_eq!(
            node_texts(&input),
            ["Address", "", "Reachable.", "Not reachable"]
        );
        assert_eq!(
            node_texts(&secret),
            ["Token", "Stored safely.", "Too short"]
        );
    }

    #[test]
    fn unknown_fields_are_refused() {
        let yaml = "steps:\n  - expect: {tree: {slot: panel, txt: x}}\n";
        assert!(serde_saphyr::from_str::<Scenario>(yaml).is_err());
    }

    #[test]
    fn messages_match_target_kind_and_payload() {
        let output = Output::Message {
            to: Target::Direct(2),
            kind: 3,
            payload: b"hi".to_vec(),
        };
        let expect = |to, kind, text: Option<&str>, hex: Option<&str>| Expect::Message {
            to,
            kind,
            text: text.map(Into::into),
            hex: hex.map(Into::into),
        };
        assert!(matches(
            &expect(Some(ToSpec::Session(2)), Some(3), Some("hi"), None),
            &output
        ));
        assert!(matches(&expect(None, None, None, Some("6869")), &output));
        assert!(!matches(
            &expect(Some(ToSpec::Word("channel".into())), None, None, None),
            &output
        ));
        assert!(!matches(&expect(None, Some(4), None, None), &output));
    }

    #[test]
    fn filtered_menu_entries_resolve_against_the_scenario_bots() {
        let peer = |session| Peer {
            session,
            name: format!("u{session}"),
            key: None,
        };
        let io = MockIo {
            name: "Music".into(),
            max_payload: MAX_PAYLOAD,
            channel: vec![peer(1), peer(2), peer(3), peer(4)],
            bots: HashMap::from([(
                2,
                BotInfo {
                    name: "mumbot".into(),
                    modules: vec!["music".into()],
                },
            )]),
            outputs: Arc::default(),
            gestures: Mutex::default(),
            claims: Mutex::default(),
        };
        let filter = UserMenuFilter {
            bot_name: None,
            bot_module: Some("music".into()),
            sessions: vec![4],
        };
        io.set_user_menu_entry_for("music", "Configure bot".into(), filter)
            .expect("accepted");
        assert_eq!(
            io.bot("music", 2).expect("ok").map(|b| b.name),
            Some("mumbot".into())
        );
        let output = lock(&io.outputs).pop().expect("an entry");
        let expect = |sessions: Option<Vec<u32>>| Expect::MenuEntry {
            label: Some("Configure bot".into()),
            sessions,
        };
        assert!(matches(&expect(Some(vec![2, 4])), &output));
        assert!(matches(&expect(None), &output));
        assert!(!matches(&expect(Some(vec![2])), &output));
    }

    #[test]
    fn panel_popover_needs_a_fresh_gesture() {
        let io = MockIo {
            name: "Poll".into(),
            max_payload: MAX_PAYLOAD,
            channel: Vec::new(),
            bots: HashMap::new(),
            outputs: Arc::default(),
            gestures: Mutex::default(),
            claims: Mutex::default(),
        };
        assert_eq!(io.open_panel_popover("poll"), Err(Error::NotPermitted));
        lock(&io.gestures).gesture("poll", Instant::now());
        assert_eq!(io.open_panel_popover("poll"), Ok(()));
        io.close_panel_popover("poll");
        let closed = lock(&io.outputs).pop().expect("closed");
        let opened = lock(&io.outputs).pop().expect("opened");
        assert!(matches(&Expect::PanelPopover { open: true }, &opened));
        assert!(matches(&Expect::PanelPopover { open: false }, &closed));
    }

    #[test]
    fn hex_round_trips() {
        assert_eq!(decode_hex("00 ff").expect("valid"), [0, 255]);
        assert!(decode_hex("0").is_err());
        assert_eq!(encode_hex(&[1, 171]), "01ab");
    }

    #[test]
    fn assets_stay_inside_the_plugin_folder() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("a.svg"), "<svg/>").expect("write");
        let files = files(&dir.path().join("plugin"));
        assert!(files.asset("../a.svg").is_none());
        assert!(files.asset("/etc/passwd").is_none());
    }

    /// An unpacked test plugin in `dir`.
    pub(crate) fn files(dir: &Path) -> PluginFiles {
        PluginFiles {
            _unpacked: None,
            dir: dir.to_owned(),
            manifest: manifest(),
        }
    }

    pub(crate) fn manifest() -> Manifest {
        Manifest::parse(
            br#"{"manifest":1,"id":"tester","name":"Tester","version":"1.0.0","api":"^1.0",
                "authors":["Tests"],"description":"A test plugin","categories":["tools"],
                "license":"MIT"}"#,
        )
        .expect("valid manifest")
    }
}
