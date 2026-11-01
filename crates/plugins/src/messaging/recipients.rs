//! Who a plugin message goes to and whose messages a plugin gets (FR-6.33, FR-6.34).
//!
//! Only users in the local user's current channel count. Blocked users are already removed by the
//! engine; whether a member is a friend comes from the caller.

use crate::manifest::Visibility;

use super::SendError;
use super::announce::{AnnounceEntry, SourceTag};

/// Another user in the local user's current channel (never the local user).
#[derive(Debug, Clone, Copy)]
pub struct Member<'a> {
    pub session: u32,
    pub is_friend: bool,
    /// What this user's last announce listed; empty if they never announced.
    pub plugins: &'a [AnnounceEntry],
}

impl Member<'_> {
    fn runs(&self, plugin: &PluginRef<'_>) -> bool {
        self.plugins
            .iter()
            .any(|e| e.matches(plugin.id, plugin.source))
    }
}

/// The local plugin a message belongs to.
#[derive(Debug, Clone, Copy)]
pub struct PluginRef<'a> {
    pub id: &'a str,
    pub source: SourceTag,
    pub visibility: Visibility,
}

impl PluginRef<'_> {
    fn allows(&self, member: &Member<'_>) -> bool {
        match self.visibility {
            Visibility::Everyone => true,
            Visibility::Friends => member.is_friend,
            Visibility::Nobody => false,
        }
    }
}

/// Who a message goes to (mirrors the WIT `peers.target`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Channel,
    Direct(u32),
}

/// Sessions that receive a message from `plugin`.
///
/// Errors: `Hidden` for visibility Nobody or a direct non-friend under Friends; `NoRecipients`
/// if nobody (allowed) in the channel runs the same plugin from the same source, or the direct
/// target is not in the channel.
pub fn select(
    plugin: &PluginRef<'_>,
    target: Target,
    members: &[Member<'_>],
) -> Result<Vec<u32>, SendError> {
    if plugin.visibility == Visibility::Nobody {
        return Err(SendError::Hidden);
    }
    match target {
        Target::Direct(session) => {
            let member = members
                .iter()
                .find(|m| m.session == session)
                .ok_or(SendError::NoRecipients)?;
            if !plugin.allows(member) {
                return Err(SendError::Hidden);
            }
            if !member.runs(plugin) {
                return Err(SendError::NoRecipients);
            }
            Ok(vec![session])
        }
        Target::Channel => {
            let sessions: Vec<u32> = members
                .iter()
                .filter(|m| plugin.allows(m) && m.runs(plugin))
                .map(|m| m.session)
                .collect();
            if sessions.is_empty() {
                Err(SendError::NoRecipients)
            } else {
                Ok(sessions)
            }
        }
    }
}

/// Why an incoming message is not delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Visibility Nobody, or a non-friend under Friends.
    Hidden,
    /// The sender is not in the channel or does not announce the same plugin from the same source.
    Mismatch,
}

/// Whether a message from `sender` (None if not in the local user's channel) reaches `plugin`.
pub fn accept(plugin: &PluginRef<'_>, sender: Option<&Member<'_>>) -> Result<(), Refusal> {
    if plugin.visibility == Visibility::Nobody {
        return Err(Refusal::Hidden);
    }
    let sender = sender.ok_or(Refusal::Mismatch)?;
    if !plugin.allows(sender) {
        return Err(Refusal::Hidden);
    }
    if !sender.runs(plugin) {
        return Err(Refusal::Mismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(source: SourceTag) -> Vec<AnnounceEntry> {
        vec![AnnounceEntry {
            id: "dice".into(),
            api_major: 1,
            source,
        }]
    }

    fn plugin(visibility: Visibility) -> PluginRef<'static> {
        PluginRef {
            id: "dice",
            source: SourceTag::Official,
            visibility,
        }
    }

    #[test]
    fn channel_send_reaches_matching_members_only() {
        let official = entries(SourceTag::Official);
        let other_repo = entries(SourceTag::Repo([1, 2, 3, 4]));
        let members = [
            Member {
                session: 1,
                is_friend: false,
                plugins: &official,
            },
            Member {
                session: 2,
                is_friend: true,
                plugins: &official,
            },
            Member {
                session: 3,
                is_friend: true,
                plugins: &other_repo,
            },
            Member {
                session: 4,
                is_friend: true,
                plugins: &[],
            },
        ];
        assert_eq!(
            select(&plugin(Visibility::Everyone), Target::Channel, &members),
            Ok(vec![1, 2])
        );
        assert_eq!(
            select(&plugin(Visibility::Friends), Target::Channel, &members),
            Ok(vec![2])
        );
        assert_eq!(
            select(&plugin(Visibility::Nobody), Target::Channel, &members),
            Err(SendError::Hidden)
        );
        assert_eq!(
            select(&plugin(Visibility::Friends), Target::Channel, &members[..1]),
            Err(SendError::NoRecipients)
        );
        assert_eq!(
            select(&plugin(Visibility::Everyone), Target::Channel, &[]),
            Err(SendError::NoRecipients)
        );
    }

    #[test]
    fn direct_send() {
        let official = entries(SourceTag::Official);
        let members = [
            Member {
                session: 1,
                is_friend: false,
                plugins: &official,
            },
            Member {
                session: 2,
                is_friend: true,
                plugins: &[],
            },
        ];
        let everyone = plugin(Visibility::Everyone);
        assert_eq!(select(&everyone, Target::Direct(1), &members), Ok(vec![1]));
        assert_eq!(
            select(&everyone, Target::Direct(2), &members),
            Err(SendError::NoRecipients)
        );
        assert_eq!(
            select(&everyone, Target::Direct(9), &members),
            Err(SendError::NoRecipients)
        );
        assert_eq!(
            select(&plugin(Visibility::Friends), Target::Direct(1), &members),
            Err(SendError::Hidden)
        );
    }

    #[test]
    fn incoming_filter() {
        let official = entries(SourceTag::Official);
        let dev = entries(SourceTag::Dev);
        let stranger = Member {
            session: 1,
            is_friend: false,
            plugins: &official,
        };
        let friend = Member {
            session: 2,
            is_friend: true,
            plugins: &official,
        };
        let dev_friend = Member {
            session: 3,
            is_friend: true,
            plugins: &dev,
        };
        assert_eq!(
            accept(&plugin(Visibility::Everyone), Some(&stranger)),
            Ok(())
        );
        assert_eq!(
            accept(&plugin(Visibility::Friends), Some(&stranger)),
            Err(Refusal::Hidden)
        );
        assert_eq!(accept(&plugin(Visibility::Friends), Some(&friend)), Ok(()));
        assert_eq!(
            accept(&plugin(Visibility::Nobody), Some(&friend)),
            Err(Refusal::Hidden)
        );
        assert_eq!(
            accept(&plugin(Visibility::Everyone), Some(&dev_friend)),
            Err(Refusal::Mismatch)
        );
        assert_eq!(
            accept(&plugin(Visibility::Everyone), None),
            Err(Refusal::Mismatch)
        );
    }
}
