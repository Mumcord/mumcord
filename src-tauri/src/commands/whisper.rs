//! Whisper and shout (issue #67): saved targets, their hold-to-talk keys, starting and stopping
//! a whisper, and the "whispers to you" marks. Targets and keys are global for all servers and
//! none exist by default (Q1.7); they are looked up on the server the user talks on.

use std::sync::{Mutex, MutexGuard, PoisonError};

use mumcord_engine::Engine;
use mumcord_engine::store;
use mumcord_engine::whisper::WhisperDenied;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use super::types::{ApiError, ApiResult, ErrorCode, ServerId, SessionId};
use crate::ptt;

/// Most saved targets; Mumble itself offers 30 voice target ids.
const MAX_TARGETS: usize = 30;
const MAX_NAME_CHARS: usize = 64;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct WhisperTarget {
    pub id: String,
    /// Plain text, chosen by the user.
    pub name: String,
    pub receivers: WhisperReceivers,
    /// Hold-to-talk key in the push-to-talk key format, e.g. `"F9"` or `"Ctrl+W"`.
    pub hotkey: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum WhisperReceivers {
    /// People by name: a whisper.
    People { names: Vec<String> },
    /// Everyone in the channel at `path` below the root: a shout.
    #[serde(rename_all = "camelCase")]
    Channel {
        path: Vec<String>,
        children: bool,
        links: bool,
        /// Only members of this ACL group.
        group: Option<String>,
    },
}

/// Why whisper keys cannot work in this session.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum WhisperKeysUnavailable {
    /// Native Wayland without the global-shortcuts portal; the X11 compatibility mode offers
    /// the keys.
    WaylandPortal,
}

/// The saved targets and whether their keys work.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct WhisperSetup {
    pub targets: Vec<WhisperTarget>,
    /// Set when no whisper key can work in this session.
    pub keys_unavailable: Option<WhisperKeysUnavailable>,
    /// Targets whose key could not be registered (e.g. another app holds it).
    pub failed_keys: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum WhisperDeniedReason {
    NoVoiceServer,
    Muted,
    ServerMuted,
    NobodyHere,
    ChannelNotFound,
    NoPermission,
    UnknownTarget,
}

/// The own whisper or shout.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum WhisperState {
    Idle,
    /// `held`: started by holding the target's key (released, it ends), else from the UI.
    #[serde(rename_all = "camelCase")]
    Active {
        target_id: String,
        held: bool,
    },
    /// The last start failed; `channel` names the channel without the Whisper permission.
    #[serde(rename_all = "camelCase")]
    Denied {
        target_id: String,
        reason: WhisperDeniedReason,
        channel: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum IncomingWhisperKind {
    /// The speaker's target names us.
    Whisper,
    /// The speaker's target is a channel we are in.
    Shout,
}

/// The own whisper started, stopped or was refused (also when started by its key).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct WhisperStateChanged {
    pub state: WhisperState,
}

/// A person started (`kind`) or stopped (`null`) whispering or shouting to us.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct IncomingWhisperChanged {
    pub server_id: ServerId,
    pub session: SessionId,
    pub kind: Option<IncomingWhisperKind>,
}

impl From<mumcord_engine::whisper::WhisperKind> for IncomingWhisperKind {
    fn from(kind: mumcord_engine::whisper::WhisperKind) -> Self {
        match kind {
            mumcord_engine::whisper::WhisperKind::Whisper => Self::Whisper,
            mumcord_engine::whisper::WhisperKind::Shout => Self::Shout,
        }
    }
}

impl From<&store::WhisperTarget> for WhisperTarget {
    fn from(t: &store::WhisperTarget) -> Self {
        Self {
            id: t.id.clone(),
            name: t.name.clone(),
            receivers: match &t.receivers {
                store::WhisperReceivers::People { names } => WhisperReceivers::People {
                    names: names.clone(),
                },
                store::WhisperReceivers::Channel {
                    path,
                    children,
                    links,
                    group,
                } => WhisperReceivers::Channel {
                    path: path.clone(),
                    children: *children,
                    links: *links,
                    group: group.clone(),
                },
            },
            hotkey: t.hotkey.clone(),
        }
    }
}

impl WhisperTarget {
    fn to_store(&self) -> store::WhisperTarget {
        let trimmed = |v: &[String]| -> Vec<String> {
            v.iter()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect()
        };
        store::WhisperTarget {
            id: self.id.clone(),
            name: self.name.trim().to_owned(),
            receivers: match &self.receivers {
                WhisperReceivers::People { names } => store::WhisperReceivers::People {
                    names: trimmed(names),
                },
                WhisperReceivers::Channel {
                    path,
                    children,
                    links,
                    group,
                } => store::WhisperReceivers::Channel {
                    path: path.clone(),
                    children: *children,
                    links: *links,
                    group: group
                        .as_deref()
                        .map(str::trim)
                        .filter(|g| !g.is_empty())
                        .map(str::to_owned),
                },
            },
            hotkey: self.hotkey.clone().filter(|k| !k.is_empty()),
        }
    }
}

/// The target being held right now, if any.
static ACTIVE: Mutex<Option<String>> = Mutex::new(None);

fn active() -> MutexGuard<'static, Option<String>> {
    ACTIVE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::InvalidInput, message)
}

/// Checks a list from the UI and returns it in store form, keys canonical.
fn validated(
    targets: &[WhisperTarget],
    ptt_key: Option<&str>,
    hotkeys: &[store::Hotkey],
) -> ApiResult<Vec<store::WhisperTarget>> {
    if targets.len() > MAX_TARGETS {
        return Err(invalid("too many whisper targets"));
    }
    let mut out: Vec<store::WhisperTarget> = Vec::with_capacity(targets.len());
    for target in targets {
        let mut t = target.to_store();
        if t.id.is_empty() || out.iter().any(|o| o.id == t.id) {
            return Err(invalid("whisper target ids must be unique"));
        }
        if t.name.is_empty() || t.name.chars().count() > MAX_NAME_CHARS {
            return Err(invalid("whisper target name must be 1 to 64 characters"));
        }
        let empty = match &t.receivers {
            store::WhisperReceivers::People { names } => names.is_empty(),
            store::WhisperReceivers::Channel { path, .. } => path.is_empty(),
        };
        if empty {
            return Err(invalid("whisper target has no receivers"));
        }
        if let Some(key) = &t.hotkey {
            let others: Vec<&str> = out.iter().filter_map(|o| o.hotkey.as_deref()).collect();
            let key = ptt::whisper::validate(key, ptt_key, hotkeys, &others)
                .map_err(|e| invalid(format!("whisper key: {e}")))?;
            t.hotkey = Some(key);
        }
        out.push(t);
    }
    Ok(out)
}

fn setup(settings: &store::Settings, failed_keys: Vec<String>) -> WhisperSetup {
    WhisperSetup {
        targets: settings.whisper_targets.iter().map(Into::into).collect(),
        keys_unavailable: ptt::whisper::unavailable(),
        failed_keys,
    }
}

/// Keys whose registration failed at the last (re)configuration.
static FAILED_KEYS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Registers the whisper keys of `settings`; at start-up and after every change.
pub fn configure_keys(app: &AppHandle, settings: &store::Settings) {
    let keys: Vec<(String, String)> = settings
        .whisper_targets
        .iter()
        .filter_map(|t| Some((t.id.clone(), t.hotkey.clone()?)))
        .collect();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let failed = ptt::whisper::configure(&app, &keys).await;
        *FAILED_KEYS.lock().unwrap_or_else(PoisonError::into_inner) = failed;
    });
}

/// Connects the whisper keys to [`start`] and [`stop`]; called once at start-up.
pub fn install_key_handler(app: &AppHandle) {
    let app = app.clone();
    ptt::whisper::set_handler(move |target_id, pressed| {
        let engine = app.state::<Engine>();
        if pressed {
            start(&app, &engine, target_id, true);
        } else {
            stop(&app, &engine, Some(target_id));
        }
    });
}

/// Starts whispering to the saved target `target_id` and tells the UI.
fn start(app: &AppHandle, engine: &Engine, target_id: &str, held: bool) -> WhisperState {
    let target = engine
        .settings()
        .ok()
        .and_then(|s| s.whisper_targets.into_iter().find(|t| t.id == target_id));
    let state = match target {
        None => WhisperState::Denied {
            target_id: target_id.to_owned(),
            reason: WhisperDeniedReason::UnknownTarget,
            channel: None,
        },
        Some(target) => match engine.whisper_start(&target.receivers) {
            Ok(()) => {
                *active() = Some(target.id.clone());
                WhisperState::Active {
                    target_id: target.id,
                    held,
                }
            }
            Err(denied) => {
                tracing::info!(reason = %denied, "whisper refused");
                denied_state(target.id, denied)
            }
        },
    };
    emit(app, state.clone());
    state
}

fn denied_state(target_id: String, denied: WhisperDenied) -> WhisperState {
    let (reason, channel) = match denied {
        WhisperDenied::NoVoiceServer => (WhisperDeniedReason::NoVoiceServer, None),
        WhisperDenied::Muted => (WhisperDeniedReason::Muted, None),
        WhisperDenied::ServerMuted => (WhisperDeniedReason::ServerMuted, None),
        WhisperDenied::NobodyHere => (WhisperDeniedReason::NobodyHere, None),
        WhisperDenied::ChannelNotFound => (WhisperDeniedReason::ChannelNotFound, None),
        WhisperDenied::NoPermission { channel } => {
            (WhisperDeniedReason::NoPermission, Some(channel))
        }
    };
    WhisperState::Denied {
        target_id,
        reason,
        channel,
    }
}

/// Ends the own whisper; with `only`, just when that target is the one being held, so
/// releasing one key never ends a whisper started by another key or the UI.
fn stop(app: &AppHandle, engine: &Engine, only: Option<&str>) {
    {
        let mut active = active();
        if active.is_none() || only.is_some_and(|id| active.as_deref() != Some(id)) {
            return;
        }
        *active = None;
    }
    engine.whisper_stop();
    emit(app, WhisperState::Idle);
}

fn emit(app: &AppHandle, state: WhisperState) {
    if let Err(e) = (WhisperStateChanged { state }).emit(app) {
        tracing::warn!(error = %e, "emitting the whisper state failed");
    }
}

/// The saved whisper targets and whether their keys work here.
#[tauri::command]
#[specta::specta]
pub fn whisper_setup(engine: State<'_, Engine>) -> ApiResult<WhisperSetup> {
    let failed = FAILED_KEYS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    Ok(setup(&engine.settings()?, failed))
}

/// Replaces the saved targets (in the given order) and re-registers their keys. A key equal to
/// the push-to-talk key or another target's key is `invalidInput`.
#[tauri::command]
#[specta::specta]
pub async fn whisper_targets_save(
    app: AppHandle,
    engine: State<'_, Engine>,
    targets: Vec<WhisperTarget>,
) -> ApiResult<WhisperSetup> {
    let mut settings = engine.settings()?;
    settings.whisper_targets = validated(&targets, settings.ptt_key.as_deref(), &settings.hotkeys)?;
    let stored = engine.update_settings(settings).await?;
    // A deleted or edited target must not keep whispering.
    stop(&app, &engine, None);
    configure_keys(&app, &stored);
    whisper_setup(engine)
}

/// Checks a key captured for a whisper target and returns its canonical form; `invalidInput`
/// if it is not a usable keyboard shortcut or is already the push-to-talk or another
/// target's key.
#[tauri::command]
#[specta::specta]
pub fn whisper_key_check(
    engine: State<'_, Engine>,
    target_id: String,
    key: String,
) -> ApiResult<String> {
    let settings = engine.settings()?;
    let others: Vec<&str> = settings
        .whisper_targets
        .iter()
        .filter(|t| t.id != target_id)
        .filter_map(|t| t.hotkey.as_deref())
        .collect();
    ptt::whisper::validate(
        &key,
        settings.ptt_key.as_deref(),
        &settings.hotkeys,
        &others,
    )
    .map_err(|e| invalid(e.to_string()))
}

/// Starts whispering or shouting to a saved target from the UI; it lasts until
/// [`whisper_stop`]. A refusal comes back as `denied` with the reason.
#[tauri::command]
#[specta::specta]
pub fn whisper_start(
    app: AppHandle,
    engine: State<'_, Engine>,
    target_id: String,
) -> ApiResult<WhisperState> {
    Ok(start(&app, &engine, &target_id, false))
}

/// Ends the own whisper or shout; talking goes back to the own channel.
#[tauri::command]
#[specta::specta]
pub fn whisper_stop(app: AppHandle, engine: State<'_, Engine>) -> ApiResult<()> {
    stop(&app, &engine, None);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(id: &str, hotkey: Option<&str>) -> WhisperTarget {
        WhisperTarget {
            id: id.into(),
            name: format!(" {id} "),
            receivers: WhisperReceivers::Channel {
                path: vec!["Raid".into()],
                children: true,
                links: false,
                group: Some("  ".into()),
            },
            hotkey: hotkey.map(str::to_owned),
        }
    }

    #[test]
    fn saved_targets_are_trimmed_and_keys_canonical() {
        let saved = validated(&[target("a", Some("control+KeyQ"))], Some("F13"), &[]).expect("ok");
        assert_eq!(saved[0].name, "a");
        assert_eq!(saved[0].hotkey.as_deref(), Some("Ctrl+Q"));
        assert_eq!(
            saved[0].receivers,
            store::WhisperReceivers::Channel {
                path: vec!["Raid".into()],
                children: true,
                links: false,
                group: None,
            }
        );
        assert_eq!(
            WhisperTarget::from(&saved[0]).hotkey.as_deref(),
            Some("Ctrl+Q")
        );
    }

    #[test]
    fn conflicting_or_broken_targets_are_refused() {
        assert!(validated(&[target("a", Some("F13"))], Some("F13"), &[]).is_err());
        assert!(
            validated(
                &[target("a", Some("F9")), target("b", Some("F9"))],
                None,
                &[]
            )
            .is_err()
        );
        assert!(validated(&[target("a", None), target("a", None)], None, &[]).is_err());
        let mut empty = target("a", None);
        empty.receivers = WhisperReceivers::People {
            names: vec![" ".into()],
        };
        assert!(validated(&[empty], None, &[]).is_err());
        assert!(validated(&[target("a", None), target("b", None)], None, &[]).is_ok());
    }
}
