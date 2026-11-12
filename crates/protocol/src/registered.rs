//! The server's list of registered users (Epic 4, issue #256).
//!
//! A client asks with a `UserList` that carries no entries; the server answers with every account it
//! has, and sends the list again after a change. There is no single-entry message shape beyond this:
//! one entry with a new name renames that account, one entry without a name removes the registration
//! and leaves the person free to connect as a guest again. Standard servers answer only users with
//! the Register permission in the root channel.
//!
//! Names and channel names are server text and personal data: untrusted input, never logged.

use std::time::SystemTime;

use crate::ban::parse_iso;
use crate::proto;

/// One registered account. `name` is `None` only in the message that removes a registration, because
/// that is how the protocol says "drop this account".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RegisteredUser {
    pub user_id: u32,
    pub name: Option<String>,
    /// When the account was last used; `None` when the server does not say (older servers send no
    /// `last_seen`, and a never-used account has none).
    pub last_seen: Option<SystemTime>,
    /// The channel the account was last seen in; `None` when the server does not say.
    pub last_channel: Option<u32>,
}

impl RegisteredUser {
    /// The entry that renames this account; the id and everything else stay as they are.
    pub fn renamed(&self, name: &str) -> Self {
        Self {
            user_id: self.user_id,
            name: Some(name.to_owned()),
            last_seen: None,
            last_channel: None,
        }
    }

    /// The entry that removes the registration; the id alone names the account.
    pub fn unregistered(&self) -> Self {
        Self {
            user_id: self.user_id,
            name: None,
            last_seen: None,
            last_channel: None,
        }
    }
}

/// Reads a server answer or a push after a change.
pub fn from_message(message: &proto::UserList) -> Vec<RegisteredUser> {
    message
        .users
        .iter()
        .map(|user| RegisteredUser {
            user_id: user.user_id,
            name: user.name.clone(),
            // murmur writes its own compact UTC form; `parse_iso` reads that and the extended one.
            last_seen: user.last_seen.as_deref().and_then(parse_iso),
            last_channel: user.last_channel,
        })
        .collect()
}

/// The query: an entry-less `UserList` asks the server to send its list.
pub fn query_message() -> proto::UserList {
    proto::UserList { users: Vec::new() }
}

/// The change for one account: a name renames it, no name removes the registration.
pub fn change_message(user: &RegisteredUser) -> proto::UserList {
    proto::UserList {
        users: vec![proto::user_list::User {
            user_id: user.user_id,
            name: user.name.clone(),
            // Only a rename or a removal goes out, so the two read-only fields stay away.
            last_seen: None,
            last_channel: None,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: Option<&str>) -> proto::user_list::User {
        proto::user_list::User {
            user_id: 18,
            name: name.map(str::to_owned),
            last_seen: Some("2026-11-06T22:40:00.000000".into()),
            last_channel: Some(2),
        }
    }

    #[test]
    fn a_server_answer_becomes_one_account_per_entry() {
        let message = proto::UserList {
            users: vec![entry(Some("Tove")), entry(None)],
        };
        let list = from_message(&message);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name.as_deref(), Some("Tove"));
        assert_eq!(list[0].last_channel, Some(2));
        assert_eq!(
            list[0]
                .last_seen
                .expect("read")
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("after the epoch")
                .as_secs(),
            1_794_004_800,
            "the server writes UTC, so 22:40 is 22:40"
        );
        assert_eq!(list[1].name, None, "an entry without a name is a removal");
    }

    #[test]
    fn the_last_seen_and_last_channel_stay_optional() {
        let mut user = entry(Some("Tove"));
        user.last_seen = None;
        user.last_channel = None;
        let list = from_message(&proto::UserList { users: vec![user] });
        assert_eq!(list[0].last_seen, None, "an old server says nothing");
        assert_eq!(list[0].last_channel, None);
    }

    #[test]
    fn a_query_carries_no_entries() {
        assert!(query_message().users.is_empty());
    }

    #[test]
    fn a_rename_sends_the_id_and_the_new_name_only() {
        let list = from_message(&proto::UserList {
            users: vec![entry(Some("Tove"))],
        });
        let renamed = change_message(&list[0].renamed("Tove L."));
        assert_eq!(renamed.users.len(), 1);
        assert_eq!(renamed.users[0].user_id, 18);
        assert_eq!(renamed.users[0].name.as_deref(), Some("Tove L."));
        assert_eq!(renamed.users[0].last_channel, None);

        let removed = change_message(&list[0].unregistered());
        assert_eq!(removed.users[0].user_id, 18);
        assert_eq!(removed.users[0].name, None, "no name removes the account");
    }

    #[test]
    fn a_change_survives_the_wire() {
        let message = change_message(&RegisteredUser {
            user_id: 7,
            name: Some("Freya".into()),
            ..Default::default()
        });
        let bytes = prost::Message::encode_to_vec(&message);
        let back: proto::UserList = prost::Message::decode(bytes.as_slice()).expect("protobuf");
        assert_eq!(
            from_message(&back).first().expect("entry").name.as_deref(),
            Some("Freya")
        );
    }
}
