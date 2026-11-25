// SPDX-License-Identifier: AGPL-3.0-or-later

//! `post-new.php` and `post.php?action=edit` for posts and pages: the
//! refusals `admin.php` and the screens make, and the pieces of the block
//! editor's page (`edit-form-blocks.php`) that need no REST response. The
//! page itself waits on its preloads.

mod assets;
pub mod blocks;
pub mod form;
pub mod page;
pub mod settings;

use hyscribe_auth::caps::post_type_exists;
use hyscribe_phpser::Value;

use crate::admin_bar::BarSite;
use crate::media::attachment::{AttachmentArgs, AttachmentSite};
use crate::menu::{Look, Screen, admin_menu};
use crate::page::query_strings;
use crate::posts::{Answer, PostsSite, lock_holder};

/// The post the block editor's page is of, once PHP's refusals pass.
#[derive(Debug, PartialEq)]
pub enum Target {
    /// `post-new.php`'s new post of a type.
    New(String),
    /// `post.php?action=edit`'s post.
    Edit(Box<hyscribe_model::Post>),
}

/// The block editor's page `post-new.php` or `post.php` (`screen`) with
/// `query` asks for: `None` for a refusal or what is not ported.
pub fn target(site: &dyn AttachmentSite, screen: &str, query: &str) -> Option<Target> {
    if screen == "post-new.php" {
        if post_new(site, query).is_some() {
            return None;
        }
        // `post_new()` passes other arguments unchecked, as not ported.
        let mut kind = "post".to_string();
        for (key, value) in query_strings(query)? {
            if key != "post_type" {
                return None;
            }
            kind = value;
        }
        return ["post", "page"]
            .contains(&kind.as_str())
            .then_some(Target::New(kind));
    }
    let args = AttachmentArgs::parse(query, None).filter(|args| args.message.is_none())?;
    let id = Value::String(args.post.into_bytes()).to_int();
    let post = u64::try_from(id).ok().and_then(|id| site.post(id))?;
    // Another user's fresh lock shows PHP's locked dialog, not ported.
    let editable = ["post", "page"].contains(&post.post_type.as_str())
        && site.can_post("edit_post", &post)
        && post.status != "trash"
        && lock_holder(site, &post).is_none();
    editable.then(|| Target::Edit(Box::new(post)))
}

/// `post-new.php` with the query `query`: the redirect of attachments to Add
/// New Media and PHP's refusals; `None` for the editor's page, the other
/// registered post types and arguments not ported.
pub fn post_new(site: &dyn PostsSite, query: &str) -> Option<Answer> {
    let mut post_type = "post".to_string();
    for (key, value) in query_strings(query)? {
        match key.as_str() {
            "post_type" => post_type = value,
            _ => return None,
        }
    }
    let ported = ["post", "page", "attachment"].contains(&post_type.as_str());
    if !ported && post_type_exists(&post_type) {
        return None;
    }
    // `admin.php` checks access before the screen runs, with the type when
    // it exists.
    let screen = Screen {
        pagenow: "post-new.php",
        typenow: if ported { &post_type } else { "" },
        taxonomy: "",
        parent_file: None,
        submenu_file: None,
        plugin_page: None,
    };
    // The theme changes the Appearance menu only, not this screen's access.
    if admin_menu(site, &screen, Look::Block).is_none() {
        return Some(Answer::from_page(None));
    }
    let cap = match post_type.as_str() {
        "post" => "edit_posts",
        "page" => "edit_pages",
        "attachment" => {
            let to = BarSite::admin_url(site, "media-new.php");
            return Some(Answer::Redirect(to));
        }
        _ => return Some(Answer::Died("Invalid post type.")),
    };
    if !site.can(cap) {
        return Some(Answer::Denied(
            "<h1>You need a higher level of permission.</h1><p>Sorry, you are not allowed to create posts as this user.</p>",
        ));
    }
    None
}

#[cfg(test)]
mod pieces_tests;
#[cfg(test)]
mod tests;
