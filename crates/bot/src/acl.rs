//! Superuser ACL edits on the local test servers (regular protocol `ACL` message).

use anyhow::{Result, bail};
use mumcord_protocol::{permissions, proto};

use crate::Bot;

impl Bot {
    /// Denies "Listen" for everyone on `channel` and its subchannels (AC-1.16). Replaces the
    /// channel's own ACL; the test channels have none. Returns once the server applied it
    /// (needs a 1.4+ server).
    pub async fn deny_listen(&self, channel: &str) -> Result<()> {
        let deny = proto::acl::ChanAcl {
            apply_here: Some(true),
            apply_subs: Some(true),
            // The server ignores entries marked as inherited.
            inherited: Some(false),
            user_id: None,
            group: Some("all".to_owned()),
            grant: Some(0),
            deny: Some(permissions::LISTEN),
        };
        self.set_acl(channel, vec![deny]).await
    }

    /// Removes the channel's own ACL entries again (back to inheriting from the parent).
    pub async fn allow_listen(&self, channel: &str) -> Result<()> {
        self.set_acl(channel, Vec::new()).await
    }

    async fn set_acl(&self, channel: &str, acls: Vec<proto::acl::ChanAcl>) -> Result<()> {
        if !self.config().superuser {
            bail!("changing ACLs needs the superuser bot");
        }
        let channel_id = self.channel_id(channel)?;
        let acl = proto::Acl {
            channel_id,
            inherit_acls: Some(true),
            groups: Vec::new(),
            acls,
            query: Some(false),
        };
        // An ACL write has no reply, but 1.4+ servers then resend the channel's state (enter
        // restrictions) to everyone, after the new ACL is in effect.
        self.send_and_await_channel_update(channel_id, "the server to apply the ACL", || {
            Ok(self.handle().send_acl(acl)?)
        })
        .await
    }
}
