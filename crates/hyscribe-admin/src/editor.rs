// SPDX-License-Identifier: AGPL-3.0-or-later

//! `post-new.php` for posts and pages: the refusals `admin.php` and the
//! screen make before the block editor's page, which is not ported.

use hyscribe_auth::caps::post_type_exists;

use crate::admin_bar::BarSite;
use crate::menu::{Look, Screen, admin_menu};
use crate::page::{THEME, query_strings};
use crate::posts::{Answer, PostsSite};

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
    };
    let look = match site.stylesheet() == THEME {
        true => Look::Block,
        false => Look::Classic("", site.template() == THEME),
    };
    if admin_menu(site, &screen, look).is_none() {
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
mod tests;
