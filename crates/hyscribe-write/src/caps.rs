// SPDX-License-Identifier: AGPL-3.0-or-later

//! `current_user_can()` inside an action, over what it reads through the
//! action's `Write`, so it sees the action's own writes.

use hyscribe_auth::caps::{CapPost, CapabilityData, User};
use hyscribe_config::Constants;
use hyscribe_db::{DbError, Meta, Write};
use hyscribe_phpser::Value;

/// What `current_user_can()` reads, as it is now in the action: the
/// options `map_meta_cap()` reads, the user's capabilities, one post and
/// the configuration's constants.
pub struct Caps {
    prefix: String,
    options: Vec<(String, Vec<u8>)>,
    user: u64,
    exists: bool,
    capabilities: Option<Vec<u8>>,
    /// The post and its trashed status, if loaded.
    post: Option<(CapPost, Option<Vec<u8>>)>,
    constants: Constants,
}

impl Caps {
    /// Reads what `user`'s checks read, with the post `post` if there is one.
    pub async fn load(
        write: &mut Write<'_>,
        user: u64,
        post: Option<u64>,
        constants: Constants,
    ) -> Result<Caps, DbError> {
        let prefix = write.prefix().to_string();
        let mut options = Vec::new();
        let roles = format!("{prefix}user_roles");
        for name in [
            roles.as_str(),
            "page_for_posts",
            "page_on_front",
            "wp_page_for_privacy_policy",
        ] {
            let value = write.option_value(name).await?.unwrap_or_default();
            options.push((name.to_string(), value));
        }
        let exists = write.user(user).await?.is_some();
        let key = format!("{prefix}capabilities");
        let capabilities = write.meta_value(Meta::User, user, &key).await?;
        let mut loaded = None;
        if let Some((id, Some(post))) = match post {
            Some(id) => Some((id, write.post(id).await?)),
            None => None,
        } {
            let trashed = write
                .meta_value(Meta::Post, id, "_wp_trash_meta_status")
                .await?;
            let post = CapPost {
                id,
                author: post.author,
                status: post.status,
                post_type: post.post_type,
                parent: post.parent,
            };
            loaded = Some((post, trashed));
        }
        Ok(Caps {
            prefix,
            options,
            user,
            exists,
            capabilities,
            post: loaded,
            constants,
        })
    }

    /// `current_user_can( $cap, ...$args )`.
    pub fn can(&self, cap: &str, args: &[Value]) -> bool {
        User::load(self, &self.prefix, self.user).has_cap(self, cap, args)
    }

    /// The capability `name` of posts of `post_type`, as `$ptype->cap` names it.
    pub fn can_type(&self, name: &str, post_type: &str) -> bool {
        let cap = match post_type {
            "page" => name.replace("posts", "pages"),
            _ => name.to_string(),
        };
        self.can(&cap, &[])
    }
}

impl CapabilityData for Caps {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        let found = self.options.iter().find(|(option, _)| option == name);
        found.map(|(_, value)| value.clone())
    }
    /// The only user meta read is the user's own capabilities.
    fn user_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        self.capabilities.clone()
    }
    fn user_exists(&self, _: u64) -> bool {
        self.exists
    }
    /// The post loaded: no other, a revision's parent in particular.
    fn post(&self, id: u64) -> Option<CapPost> {
        let found = self.post.as_ref().filter(|(post, _)| post.id == id);
        found.map(|(post, _)| post.clone())
    }
    /// The only post meta read is `_wp_trash_meta_status`.
    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        self.post.as_ref().and_then(|(_, trashed)| trashed.clone())
    }
    fn comment_post(&self, _: u64) -> Option<u64> {
        None
    }
    fn term_taxonomy(&self, _: u64) -> Option<String> {
        None
    }
    fn constant(&self, name: &str) -> bool {
        self.constants.defined(name)
    }
}

#[cfg(test)]
mod tests;
