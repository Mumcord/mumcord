//! Slash commands (FR-6.43). The registry is rebuilt from the installed plugins on every call
//! instead of being kept in step with install, remove, enable and permission changes: it is
//! tiny, and this way it can never go stale.

use crate::chat::{CommandRegistry, Resolution};
use crate::host::RunState;
use crate::manifest::Command;
use crate::wit::events::{ActionSource, ActionValue};

use super::{PluginService, ServiceError, UiInput};

const COMMANDS_PERMISSION: &str = "commands";

/// One declared command of an active plugin, for autocomplete and the plugin's settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCommand {
    pub plugin: String,
    pub plugin_name: String,
    pub name: String,
    pub description: String,
    pub usage: Option<String>,
    /// Who owns the name instead ("Command taken by <plugin>"); `None` if this plugin owns it.
    pub taken_by: Option<String>,
}

/// What happened to a typed line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOutcome {
    /// A plugin received the command; the line must not be posted.
    Plugin,
    /// Not a plugin command: the chat box sends the line as normal text.
    Unknown,
}

/// A plugin that may take part in the registry.
struct Candidate<'a> {
    id: &'a str,
    name: &'a str,
    installed_at: i64,
    active: bool,
    commands: &'a [Command],
}

/// Active plugins only, in install order ("first installed wins").
fn build(mut candidates: Vec<Candidate<'_>>) -> CommandRegistry {
    candidates.sort_by_key(|c| c.installed_at);
    let mut registry = CommandRegistry::new(&[]);
    for candidate in candidates.iter().filter(|c| c.active) {
        registry.register(candidate.id, candidate.name, candidate.commands);
    }
    registry
}

impl PluginService {
    /// Running plugins that hold `commands`, and their ids in install order.
    fn command_registry(&self) -> Result<(CommandRegistry, Vec<String>), ServiceError> {
        let records = self.store().plugins()?;
        let candidates: Vec<Candidate<'_>> = records
            .iter()
            .filter(|r| r.removed_at.is_none())
            .filter_map(|record| {
                let manifest = self.host.manifest(&record.id)?;
                Some(Candidate {
                    id: &record.id,
                    name: &manifest.name,
                    installed_at: record.installed_at,
                    active: record.enabled
                        && record.granted.iter().any(|p| p == COMMANDS_PERMISSION)
                        && matches!(self.host.state(&record.id), Some(RunState::Running)),
                    commands: &manifest.commands,
                })
            })
            .collect();
        let mut active: Vec<&Candidate<'_>> = candidates.iter().filter(|c| c.active).collect();
        active.sort_by_key(|c| c.installed_at);
        let ids = active.iter().map(|c| c.id.to_owned()).collect();
        Ok((build(candidates), ids))
    }

    /// Every command of the active plugins, owned ones and those taken by another plugin.
    pub fn commands(&self) -> Result<Vec<PluginCommand>, ServiceError> {
        let (registry, ids) = self.command_registry()?;
        let mut out = Vec::new();
        for id in &ids {
            let Some(manifest) = self.host.manifest(id) else {
                continue;
            };
            for command in &manifest.commands {
                out.push(PluginCommand {
                    plugin: id.clone(),
                    plugin_name: manifest.name.clone(),
                    name: command.name.clone(),
                    description: command.description.clone(),
                    usage: command.usage.clone(),
                    taken_by: registry.taken_by(id, &command.name),
                });
            }
        }
        Ok(out)
    }

    /// A line the user typed: a plugin command goes to its plugin as a `ui-action` with source
    /// `command` (action = command name, value = the rest of the line) and opens the command
    /// gesture window.
    pub fn run_command(&self, line: &str) -> Result<CommandOutcome, ServiceError> {
        match self.command_registry()?.0.resolve(line) {
            Resolution::Plugin { plugin, action } => {
                self.ui_input(
                    &plugin,
                    UiInput::Action {
                        source: ActionSource::Command,
                        action: action.action,
                        value: ActionValue::Text(action.text),
                    },
                );
                Ok(CommandOutcome::Plugin)
            }
            Resolution::Builtin | Resolution::Unknown => Ok(CommandOutcome::Unknown),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roll() -> Vec<Command> {
        vec![Command {
            name: "roll".into(),
            description: "Roll dice".into(),
            usage: Some("/roll 2d6".into()),
        }]
    }

    fn candidate<'a>(id: &'a str, at: i64, active: bool, commands: &'a [Command]) -> Candidate<'a> {
        Candidate {
            id,
            name: id,
            installed_at: at,
            active,
            commands,
        }
    }

    fn owner(registry: &CommandRegistry, line: &str) -> Option<String> {
        match registry.resolve(line) {
            Resolution::Plugin { plugin, .. } => Some(plugin),
            _ => None,
        }
    }

    #[test]
    fn the_first_installed_plugin_wins_whatever_the_row_order() {
        let commands = roll();
        let registry = build(vec![
            candidate("late", 20, true, &commands),
            candidate("early", 10, true, &commands),
        ]);
        assert_eq!(owner(&registry, "/roll 2d6"), Some("early".into()));
        assert_eq!(registry.taken_by("late", "roll"), Some("early".into()));
    }

    #[test]
    fn inactive_plugins_neither_own_nor_block_a_command() {
        let commands = roll();
        let registry = build(vec![
            candidate("off", 10, false, &commands),
            candidate("on", 20, true, &commands),
        ]);
        assert_eq!(owner(&registry, "/roll"), Some("on".into()));
        let registry = build(vec![candidate("off", 10, false, &commands)]);
        assert_eq!(registry.resolve("/roll"), Resolution::Unknown);
    }

    #[test]
    fn plain_text_and_unknown_commands_are_not_handled() {
        let commands = roll();
        let registry = build(vec![candidate("dice", 10, true, &commands)]);
        assert_eq!(registry.resolve("roll 2d6"), Resolution::Unknown);
        assert_eq!(registry.resolve("/shrug"), Resolution::Unknown);
    }
}
