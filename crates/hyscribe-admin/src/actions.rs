// SPDX-License-Identifier: AGPL-3.0-or-later

//! The checks admin actions make before they write, as WordPress makes
//! them: `check_admin_referer()`, `check_ajax_referer()` and the
//! capability checks of each action. An action that passes them all goes
//! to its write's handler in the HTTP route ([`action`] names it), or is
//! answered `501` when it has none, as is one whose next step is not ported.

use hyscribe_auth::caps::{CapPost, CapabilityData, post_type_exists, taxonomy_cap};
use hyscribe_kses::{esc_html, esc_url, esc_url_raw, sanitize_key};
use hyscribe_phpser::{Key, Value, addslashes, lookup, php_trim, remove_query_arg};
use hyscribe_render::formatting::sanitize_text_field;

use crate::users::role_names;

/// What the checks read of the site and the current user.
pub trait ActionSite: CapabilityData {
    /// `current_user_can( $cap, ...$args )`.
    fn current_user_can(&self, cap: &str, args: &[Value]) -> bool;
    /// `wp_verify_nonce( $nonce, $action )` for the current user.
    fn verify_nonce(&self, nonce: &[u8], action: &str) -> bool;
    /// `wp_validate_redirect( $location, '' )`.
    fn validate_redirect(&self, location: &[u8]) -> Vec<u8>;
    /// The comment's `comment_approved`, `None` without the comment.
    fn comment_approved(&self, id: u64) -> Option<Vec<u8>>;
    /// The roles as stored, `{prefix}user_roles`.
    fn user_roles(&self) -> Option<Vec<u8>>;
}

/// The request an action is read from.
pub struct Asked<'a> {
    /// `$_GET`, unslashed.
    pub get: &'a [(Key, Value)],
    /// `$_POST`, unslashed.
    pub post: &'a [(Key, Value)],
    /// The `Referer` header.
    pub referer: Option<&'a [u8]>,
    /// `$_SERVER['REQUEST_URI']`, unslashed.
    pub uri: &'a [u8],
    /// `home_url()`.
    pub home: &'a str,
    /// `get_current_user_id()`.
    pub user: u64,
}

impl Asked<'_> {
    /// `$_REQUEST[ $name ]`: `$_POST`'s before `$_GET`'s.
    pub fn request(&self, name: &str) -> Option<&Value> {
        lookup(self.post, name).or_else(|| lookup(self.get, name))
    }

    fn get(&self, name: &str) -> Option<&Value> {
        lookup(self.get, name)
    }

    fn post(&self, name: &str) -> Option<&Value> {
        lookup(self.post, name)
    }

    /// `! empty( $_REQUEST[ $name ] )`.
    fn filled(&self, name: &str) -> bool {
        self.request(name).is_some_and(|value| !value.is_empty())
    }

    /// `(int) $_REQUEST[ $name ]`, 0 when not set.
    fn int(&self, name: &str) -> i64 {
        self.request(name).map_or(0, Value::to_int)
    }

    /// `$_REQUEST[ $name ]`, if a string equal to `text`.
    fn is(&self, name: &str, text: &str) -> bool {
        self.request(name).and_then(Value::as_bytes) == Some(text.as_bytes())
    }

    /// `! empty( $_REQUEST['action'] ) ? sanitize_text_field( ... ) : ''`.
    fn action(&self) -> Vec<u8> {
        self.request("action")
            .filter(|action| !action.is_empty())
            .and_then(Value::as_bytes)
            .map(sanitize_text_field)
            .unwrap_or_default()
    }

    /// `wp_get_referer()`.
    fn referer(&self, site: &dyn ActionSite) -> Option<Vec<u8>> {
        self.wp_get_referer(&|location| site.validate_redirect(location))
    }

    /// `wp_get_referer()`, with `validate` as `wp_validate_redirect( $location, '' )`.
    pub fn wp_get_referer(&self, validate: &dyn Fn(&[u8]) -> Vec<u8>) -> Option<Vec<u8>> {
        let given = self
            .request("_wp_http_referer")
            .filter(|referer| !referer.is_empty())
            .and_then(Value::as_bytes);
        let raw = given.or(self
            .referer
            .filter(|r| !Value::String(r.to_vec()).is_empty()))?;
        let here = [self.home.as_bytes(), self.uri].concat();
        let validated = (raw != self.uri && raw != here).then(|| validate(raw));
        validated.filter(|referer| !referer.is_empty())
    }

    /// `WP_List_Table::current_action()` when it is not empty, `delete_all`
    /// for a list that empties a status.
    fn current_action(&self, delete_all: bool) -> Option<Value> {
        if delete_all
            && ["delete_all", "delete_all2"]
                .iter()
                .any(|n| self.request(n).is_some())
        {
            return Some(Value::String(b"delete_all".to_vec()));
        }
        if self.filled("filter_action") {
            return None;
        }
        self.request("action")
            .filter(|action| action.as_bytes() != Some(b"-1") && !action.is_empty())
            .cloned()
    }
}

/// How a refusal is sent.
#[derive(Debug, PartialEq)]
pub enum Body {
    /// `wp_die()`'s page, with its title.
    Page(&'static str, String),
    /// `wp_die()` for an Ajax request: the message alone.
    Text(String),
    /// `wp_send_json_error()`.
    Json(&'static str),
}

/// A refusal: its status and what is sent.
#[derive(Debug, PartialEq)]
pub struct Refusal {
    /// The HTTP status.
    pub status: u16,
    /// What is sent.
    pub body: Body,
}

/// `wp_die()`'s default title.
const ERROR: &str = "WordPress &rsaquo; Error";

fn died(status: u16, message: impl Into<String>) -> Refusal {
    Refusal {
        status,
        body: Body::Page(ERROR, message.into()),
    }
}

/// `wp_die( '<h1>You need a higher level of permission.</h1><p>...</p>', 403 )`.
fn higher(message: &str) -> Refusal {
    died(
        403,
        format!("<h1>You need a higher level of permission.</h1><p>{message}</p>"),
    )
}

fn ajax(status: u16, body: &str) -> Refusal {
    Refusal {
        status,
        body: Body::Text(body.into()),
    }
}

/// The checks' outcome: `Ok` once they pass, for the `501` of a write not
/// ported.
pub type Checked = Result<(), Refusal>;

/// `wp_nonce_ays( $action )`.
fn nonce_ays(site: &dyn ActionSite, asked: &Asked) -> Refusal {
    let mut html = String::from("The link you followed has expired.");
    if let Some(referer) = asked.referer(site) {
        let back = site.validate_redirect(&esc_url_raw(&remove_query_arg(&[b"updated"], &referer)));
        html.push_str(&format!(
            "</p><p><a href=\"{}\">Please try again.</a>",
            String::from_utf8_lossy(&esc_url(&back))
        ));
    }
    Refusal {
        status: 403,
        body: Body::Page("An error occurred.", html),
    }
}

/// `wp_verify_nonce()` of the value given, `(string)` cast.
fn verified(site: &dyn ActionSite, nonce: Option<&Value>, action: &str) -> bool {
    nonce.is_some_and(|nonce| site.verify_nonce(&nonce.to_php_string(), action))
}

/// `check_admin_referer( $action, $field )`.
fn admin_referer(site: &dyn ActionSite, asked: &Asked, action: &str, field: &str) -> Checked {
    if verified(site, asked.request(field), action) {
        Ok(())
    } else {
        Err(nonce_ays(site, asked))
    }
}

/// `check_admin_referer( $action )`.
fn referer(site: &dyn ActionSite, asked: &Asked, action: &str) -> Checked {
    admin_referer(site, asked, action, "_wpnonce")
}

/// `check_ajax_referer( $action, $field )`: `-1` with a `403`.
fn ajax_referer(
    site: &dyn ActionSite,
    asked: &Asked,
    action: &str,
    field: Option<&str>,
) -> Checked {
    let nonce = field
        .and_then(|field| asked.request(field))
        .or_else(|| asked.request("_ajax_nonce"))
        .or_else(|| asked.request("_wpnonce"));
    if verified(site, nonce, action) {
        Ok(())
    } else {
        Err(ajax(403, "-1"))
    }
}

/// `current_user_can( $cap, $id )`.
fn can_for(site: &dyn ActionSite, cap: &str, id: i64) -> bool {
    site.current_user_can(cap, &[Value::Int(id)])
}

/// `(array) $value`'s items.
fn items(value: &Value) -> Vec<Value> {
    match value {
        Value::Array(entries) => entries.iter().map(|(_, item)| item.clone()).collect(),
        value => vec![value.clone()],
    }
}

/// The post's `_edit_lock`, whose check is not ported.
fn locked(site: &dyn ActionSite, id: i64) -> bool {
    let id = u64::try_from(id).unwrap_or_default();
    site.post_meta(id, "_edit_lock")
        .is_some_and(|lock| !Value::String(lock).is_empty())
}

/// The checks of an action on the admin screen `screen` (hs-admin's path
/// without `/hs-admin/`); `None` for a request that is not one.
pub fn admin(site: &dyn ActionSite, screen: &str, asked: &Asked, post: bool) -> Option<Checked> {
    // `admin.php`'s `set_screen_options()`, before any screen's own.
    if screen_options(asked) {
        return Some(admin_referer(
            site,
            asked,
            "screen-options-nonce",
            "screenoptionnonce",
        ));
    }
    let checked = match screen {
        "edit" => posts_list(site, asked)?,
        "post" => post_action(site, asked, post)?,
        "edit-tags" => terms_list(site, asked)?,
        "edit-comments" => comments_list(site, asked)?,
        "users" => users_list(site, asked)?,
        "user-new" => user_new(site, asked)?,
        "profile" | "user-edit" => user_edit(site, asked, screen == "profile")?,
        "options" => options(site, asked),
        "options-permalink" => permalinks(site, asked)?,
        "themes" => themes(site, asked)?,
        _ => return None,
    };
    Some(checked)
}

/// `admin.php`'s refusal of a screen the menu does not offer the user.
fn no_access() -> Refusal {
    died(403, "Sorry, you are not allowed to access this page.")
}

/// `edit.php`'s bulk actions on posts and pages.
fn posts_list(site: &dyn ActionSite, asked: &Asked) -> Option<Checked> {
    let doaction = asked.current_action(true)?;
    let post_type = match asked.request("post_type").map(Value::to_php_string) {
        None => "post",
        Some(name) if name == b"page" => "page",
        Some(name) if name == b"post" => "post",
        Some(_) => return Some(Ok(())),
    };
    let edit = if post_type == "page" {
        "edit_pages"
    } else {
        "edit_posts"
    };
    Some(posts_bulk(site, asked, &doaction, edit))
}

/// The status check of `wp_trash_post()` or `wp_untrash_post()`, which
/// fail without writing for a post in, or not in, the Trash.
type StatusCheck = fn(&str) -> Option<&'static str>;

fn trash_check(status: &str) -> Option<&'static str> {
    (status == "trash").then_some("Error in moving the item to Trash.")
}

fn untrash_check(status: &str) -> Option<&'static str> {
    (status != "trash").then_some("Error in restoring the item from Trash.")
}

/// `get_post( $id )`'s status, empty without the post.
fn post_status(site: &dyn ActionSite, id: i64) -> String {
    let post = u64::try_from(id).ok().and_then(|id| site.post(id));
    post.map(|post| post.status).unwrap_or_default()
}

fn posts_bulk(site: &dyn ActionSite, asked: &Asked, doaction: &Value, edit: &str) -> Checked {
    // The menu offers the posts list to who may edit posts.
    if !site.current_user_can("edit_posts", &[]) {
        return Err(no_access());
    }
    if !site.current_user_can(edit, &[]) {
        return Err(higher(
            "Sorry, you are not allowed to edit posts in this post type.",
        ));
    }
    // Back-compat redirects to a post's comments.
    if ["p", "attachment_id", "page_id"]
        .iter()
        .any(|n| asked.filled(n))
    {
        return Ok(());
    }
    referer(site, asked, "bulk-posts")?;
    // `ids` and `post` that PHP refuses with a fatal error are not ported,
    // nor emptying a status (`delete_all`), which no action below matches.
    let ids = match (
        asked.request("media"),
        asked.request("ids"),
        asked.request("post"),
    ) {
        (Some(media), _, _) => items(media),
        (None, Some(Value::String(ids)), _) => {
            let parts = ids.split(|&b| b == b',');
            parts.map(|id| Value::String(id.to_vec())).collect()
        }
        (None, None, Some(Value::Array(posts))) => items(&Value::Array(posts.clone())),
        _ => return Ok(()),
    };
    let first = match ids.first() {
        Some(id) if asked.request("media").is_none_or(|media| !media.is_empty()) => id.to_int(),
        _ => return Ok(()),
    };
    let (denied, check): (&str, Option<StatusCheck>) = match doaction.as_bytes() {
        Some(b"trash") => (
            "Sorry, you are not allowed to move this item to the Trash.",
            Some(trash_check),
        ),
        Some(b"untrash") => (
            "Sorry, you are not allowed to restore this item from the Trash.",
            Some(untrash_check),
        ),
        Some(b"delete") => ("Sorry, you are not allowed to delete this item.", None),
        _ => return Ok(()),
    };
    // The first post is written unless refused, or locked and skipped,
    // which is not ported.
    if !can_for(site, "delete_post", first) {
        return Err(died(500, denied));
    }
    if doaction.as_bytes() == Some(b"trash") && locked(site, first) {
        return Ok(());
    }
    let error = check.and_then(|check| check(&post_status(site, first)));
    error.map_or(Ok(()), |error| Err(died(500, error)))
}

/// `post.php`'s trash, restore and delete; `None` for the edit screen.
fn post_action(site: &dyn ActionSite, asked: &Asked, post: bool) -> Option<Checked> {
    let title = "Sorry, you are not allowed to edit this item.";
    let mismatch = |message: &str| Refusal {
        status: 400,
        body: Body::Page(title, message.into()),
    };
    let (get_id, post_id) = (asked.get("post"), asked.post("post_ID"));
    if let (Some(get_id), Some(post_id)) = (get_id, post_id)
        && get_id.to_int() != post_id.to_int()
    {
        return Some(Err(mismatch("A post ID mismatch has been detected.")));
    }
    let id = get_id.or(post_id).map_or(0, Value::to_int);
    let found = u64::try_from(id)
        .ok()
        .filter(|&id| id != 0)
        .and_then(|id| site.post(id));
    let given_type = asked.post("post_type");
    if let (Some(given), Some(found)) = (given_type, &found)
        && given.as_bytes() != Some(found.post_type.as_bytes())
    {
        return Some(Err(mismatch("A post type mismatch has been detected.")));
    }
    let action = if asked.post("deletepost").is_some() {
        b"delete".to_vec()
    } else {
        asked.action()
    };
    let (gone, denied, check): (&str, &str, Option<StatusCheck>) = match action.as_slice() {
        b"trash" => (
            "The item you are trying to move to the Trash no longer exists.",
            "Sorry, you are not allowed to move this item to the Trash.",
            Some(trash_check),
        ),
        b"untrash" => (
            "The item you are trying to restore from the Trash no longer exists.",
            "Sorry, you are not allowed to restore this item from the Trash.",
            Some(untrash_check),
        ),
        b"delete" => (
            "This item has already been deleted.",
            "Sorry, you are not allowed to delete this item.",
            None,
        ),
        b"post-quickdraft-save" if post => return Some(quickdraft(site, asked, found, id)),
        _ => return post.then_some(Ok(())),
    };
    let nonce = format!("{}-post_{id}", String::from_utf8_lossy(&action));
    Some(referer(site, asked, &nonce).and_then(|()| {
        let found = found.ok_or_else(|| died(410, gone))?;
        if !post_type_exists(&found.post_type) {
            return Err(died(400, "Invalid post type."));
        }
        if !can_for(site, "delete_post", id) {
            return Err(died(403, denied));
        }
        // The lock's refusal is not ported.
        if action == b"trash" && locked(site, id) {
            return Ok(());
        }
        let error = check.and_then(|check| check(&found.status));
        error.map_or(Ok(()), |error| Err(died(500, error)))
    }))
}

/// `trim( $_POST[ $name ] )` of the Quick Draft form, on the slashed value
/// as PHP trims it; `None` for an array, PHP's fatal error.
pub fn quickdraft_field(asked: &Asked, name: &str) -> Option<Vec<u8>> {
    let given = match asked.post(name) {
        Some(Value::Array(_)) => return None,
        given => given.map(Value::to_php_string).unwrap_or_default(),
    };
    Some(php_trim(&addslashes(&given)).to_vec())
}

/// Whether the Quick Draft form has a title or content, which a draft
/// needs; `None` for a field PHP cannot trim.
pub fn quickdraft_filled(asked: &Asked) -> Option<bool> {
    let title = quickdraft_field(asked, "post_title")?;
    let content = quickdraft_field(asked, "content")?;
    Some(!Value::String(title).is_empty() || !Value::String(content).is_empty())
}

/// `post.php`'s Quick Draft: `exit` for who may not create posts, then,
/// with a valid nonce, the referer of its post's type and, once the form is
/// filled, `edit_post()`'s capability check. The rest answers with the form.
fn quickdraft(site: &dyn ActionSite, asked: &Asked, found: Option<CapPost>, id: i64) -> Checked {
    if !site.current_user_can("edit_posts", &[]) {
        return Err(ajax(200, ""));
    }
    if !verified(site, asked.request("_wpnonce"), "add-post") {
        return Ok(());
    }
    let kind = found.map(|post| post.post_type).unwrap_or_default();
    referer(site, asked, &format!("add-{kind}"))?;
    match quickdraft_filled(asked) {
        Some(true) if !can_for(site, "edit_post", id) => {
            Err(died(500, "Sorry, you are not allowed to edit this post."))
        }
        _ => Ok(()),
    }
}

/// `edit-tags.php`'s actions on categories and tags.
fn terms_list(site: &dyn ActionSite, asked: &Asked) -> Option<Checked> {
    let doaction = if asked.is("action", "delete") && asked.request("delete_tags").is_some() {
        b"bulk-delete".to_vec()
    } else {
        asked.current_action(false)?.to_php_string()
    };
    // Without a taxonomy the screen is not ported.
    let taxonomy = asked.request("taxonomy").and_then(Value::as_bytes);
    let taxonomy = match taxonomy {
        Some(b"category") => "category",
        Some(b"post_tag") => "post_tag",
        _ => return Some(Ok(())),
    };
    let manage = taxonomy_cap(taxonomy, "manage_terms").unwrap_or_default();
    if !site.current_user_can(&manage, &[]) {
        return Some(Err(higher(
            "Sorry, you are not allowed to manage terms in this taxonomy.",
        )));
    }
    let checked = match doaction.as_slice() {
        // `edit_terms` and `delete_terms` map to `manage_terms`, checked above,
        // for both taxonomies: adding and bulk deleting check only the nonce.
        b"add-tag" => admin_referer(site, asked, "add-tag", "_wpnonce_add-tag"),
        b"delete" if asked.request("tag_ID").is_some() => {
            let id = asked.int("tag_ID");
            referer(site, asked, &format!("delete-tag_{id}")).and_then(|()| {
                match can_for(site, "delete_term", id) {
                    true => Ok(()),
                    false => Err(higher("Sorry, you are not allowed to delete this item.")),
                }
            })
        }
        b"bulk-delete" => referer(site, asked, "bulk-tags"),
        b"editedtag" => {
            let id = asked.post("tag_ID").map_or(0, Value::to_int);
            referer(site, asked, &format!("update-tag_{id}")).and_then(|()| {
                match can_for(site, "edit_term", id) {
                    true => Ok(()),
                    false => Err(higher("Sorry, you are not allowed to edit this item.")),
                }
            })
        }
        _ => Ok(()),
    };
    Some(checked)
}

/// `edit-comments.php`'s bulk actions, which refuse nothing after the
/// nonce: comments the user may not edit are left out.
fn comments_list(site: &dyn ActionSite, asked: &Asked) -> Option<Checked> {
    asked.current_action(true)?;
    if !site.current_user_can("edit_posts", &[]) {
        return Some(Err(no_access()));
    }
    Some(referer(site, asked, "bulk-comments"))
}

/// The user IDs of `$_REQUEST['users']`, `intval()`ed.
pub fn user_ids(asked: &Asked) -> Vec<i64> {
    asked
        .request("users")
        .map(|users| items(users).iter().map(Value::to_int).collect())
        .unwrap_or_default()
}

/// `WP_Users_List_Table::current_action()`: `promote` for the role change.
fn users_action(asked: &Asked) -> Option<Vec<u8>> {
    if asked.request("changeit").is_some() {
        return Some(b"promote".to_vec());
    }
    asked
        .current_action(false)
        .as_ref()
        .map(Value::to_php_string)
}

/// The action of a request whose checks passed, as its write's handler is
/// registered: `screen-options` for the Screen Options form, the users
/// list's `current_action()`, else `$_REQUEST['action']`.
pub fn action(screen: &str, asked: &Asked) -> Vec<u8> {
    match screen {
        "admin-ajax" => asked.action(),
        _ if screen_options(asked) => b"screen-options".to_vec(),
        "users" => users_action(asked).unwrap_or_default(),
        _ => asked.action(),
    }
}

/// Whether the Screen Options form is posted: `$_POST['wp_screen_options']`, an array.
fn screen_options(asked: &Asked) -> bool {
    matches!(asked.post("wp_screen_options"), Some(Value::Array(_)))
}

/// `users.php`'s actions.
fn users_list(site: &dyn ActionSite, asked: &Asked) -> Option<Checked> {
    let doaction = users_action(asked)?;
    if !site.current_user_can("list_users", &[]) {
        return Some(Err(higher("Sorry, you are not allowed to list users.")));
    }
    let me = i64::try_from(asked.user).unwrap_or_default();
    let checked = match doaction.as_slice() {
        b"promote" => referer(site, asked, "bulk-users").and_then(|()| promote(site, asked, me)),
        b"dodelete" => referer(site, asked, "delete-users").and_then(|()| {
            if !asked.filled("users") || !asked.filled("delete_option") {
                return Ok(());
            }
            // Each user's `delete_user` maps to this on a single site.
            match site.current_user_can("delete_users", &[]) {
                true => Ok(()),
                false => Err(died(403, "Sorry, you are not allowed to delete users.")),
            }
        }),
        b"resetpassword" => referer(site, asked, "bulk-users").and_then(|()| {
            // `edit_user` maps to nothing for oneself and to `edit_users` for
            // another, whom PHP checks before it writes.
            let others = user_ids(asked).into_iter().any(|id| id != me);
            match others && !site.current_user_can("edit_users", &[]) {
                true => Err(died(500, "Sorry, you are not allowed to edit this user.")),
                false => Ok(()),
            }
        }),
        b"remove" => {
            referer(site, asked, "bulk-users").and(Err(died(400, "You cannot remove users.")))
        }
        b"doremove" => {
            referer(site, asked, "remove-users").and(Err(died(400, "You cannot remove users.")))
        }
        b"delete" => referer(site, asked, "bulk-users"),
        _ => Ok(()),
    };
    Some(checked)
}

/// The role change of `users.php`.
fn promote(site: &dyn ActionSite, asked: &Asked, me: i64) -> Checked {
    if !site.current_user_can("promote_users", &[]) {
        return Err(died(403, "Sorry, you are not allowed to edit this user."));
    }
    if !asked.filled("users") {
        return Ok(());
    }
    let role = match asked.request("new_role") {
        Some(Value::String(role)) => role.clone(),
        None => Vec::new(),
        // An array is a fatal error in PHP.
        Some(_) => return Ok(()),
    };
    let roles = role_names(&site.user_roles().unwrap_or_default());
    let editable = role == b"none" || roles.iter().any(|(slug, _)| slug.as_bytes() == role);
    if Value::String(role.clone()).is_empty() || !editable {
        return Err(died(
            403,
            "Sorry, you are not allowed to give users that role.",
        ));
    }
    // Each user's `promote_user` maps to `promote_users`, checked above.
    for id in user_ids(asked) {
        if id != me {
            return Ok(());
        }
        if role == b"none" {
            return Err(died(403, "Sorry, you cannot remove your own role."));
        }
    }
    Ok(())
}

/// `user-new.php`'s actions.
fn user_new(site: &dyn ActionSite, asked: &Asked) -> Option<Checked> {
    let (nonce, field) = if asked.is("action", "adduser") {
        ("add-user", "_wpnonce_add-user")
    } else if asked.is("action", "createuser") {
        ("create-user", "_wpnonce_create-user")
    } else {
        return None;
    };
    if !site.current_user_can("create_users", &[]) {
        return Some(Err(higher("Sorry, you are not allowed to create users.")));
    }
    Some(admin_referer(site, asked, nonce, field))
}

/// `user-edit.php`'s and `profile.php`'s actions.
fn user_edit(site: &dyn ActionSite, asked: &Asked, profile: bool) -> Option<Checked> {
    let me = asked.user;
    let given = asked
        .request("user_id")
        .filter(|id| !id.is_empty())
        .map_or(0, |id| id.to_int().unsigned_abs());
    let own = profile || given == me;
    let dismiss = asked.get("dismiss").and_then(Value::as_bytes)
        == Some(format!("{me}_new_email").as_bytes());
    // A confirmed email change is not ported.
    let confirmed = own && asked.get("newuseremail").is_some();
    if asked.action() != b"update" && !(own && dismiss) && !confirmed {
        return None;
    }
    let id = if given == 0 && own { me } else { given };
    if id == 0 || !site.user_exists(id) {
        return Some(Err(died(500, "Invalid user ID.")));
    }
    if confirmed {
        return Some(Ok(()));
    }
    if own && dismiss {
        return Some(referer(site, asked, &format!("dismiss-{me}_new_email")));
    }
    let id = i64::try_from(id).unwrap_or_default();
    Some(
        referer(site, asked, &format!("update-user_{id}")).and_then(|()| {
            match can_for(site, "edit_user", id) {
                true => Ok(()),
                false => Err(died(500, "Sorry, you are not allowed to edit this user.")),
            }
        }),
    )
}

/// The option pages `options.php` saves.
const OPTION_PAGES: [&str; 8] = [
    "general",
    "discussion",
    "media",
    "reading",
    "writing",
    "misc",
    "options",
    "privacy",
];

/// `options.php`, which saves the Settings screens.
fn options(site: &dyn ActionSite, asked: &Asked) -> Checked {
    if !site.current_user_can("manage_options", &[]) {
        return Err(higher(
            "Sorry, you are not allowed to manage options for this site.",
        ));
    }
    if asked.get("adminhash").is_some_and(|hash| !hash.is_empty()) {
        return Ok(());
    }
    if asked.get("dismiss").and_then(Value::as_bytes) == Some(b"new_admin_email") {
        return referer(site, asked, "dismiss-1-new_admin_email");
    }
    if asked.action() != b"update" {
        return Ok(());
    }
    let given = asked
        .request("option_page")
        .filter(|page| !page.is_empty())
        .and_then(Value::as_bytes)
        .map(|page| sanitize_text_field(&addslashes(page)))
        .filter(|page| !Value::String(page.clone()).is_empty());
    let page = given.unwrap_or_else(|| b"options".to_vec());
    let nonce = if page == b"options" && asked.post("option_page").is_none() {
        "update-options".to_string()
    } else {
        format!("{}-options", String::from_utf8_lossy(&page))
    };
    referer(site, asked, &nonce)?;
    if !OPTION_PAGES.iter().any(|known| known.as_bytes() == page) {
        return Err(died(
            500,
            format!(
                "<strong>Error:</strong> The <code>{}</code> options page is not in the allowed options list.",
                esc_html(&page)
            ),
        ));
    }
    Ok(())
}

/// The Permalinks screen's save.
fn permalinks(site: &dyn ActionSite, asked: &Asked) -> Option<Checked> {
    if asked.post("permalink_structure").is_none() && asked.post("category_base").is_none() {
        return None;
    }
    if !site.current_user_can("manage_options", &[]) {
        return Some(Err(no_access()));
    }
    Some(referer(site, asked, "update-permalink"))
}

/// `themes.php`'s actions: only the nonce of a theme's activation is
/// checked; which themes exist is not ported.
fn themes(site: &dyn ActionSite, asked: &Asked) -> Option<Checked> {
    let action = asked.get("action")?;
    // The menu offers the screen to who may do either.
    if !site.current_user_can("switch_themes", &[])
        && !site.current_user_can("edit_theme_options", &[])
    {
        return Some(Err(no_access()));
    }
    if !site.current_user_can("switch_themes", &[]) || action.as_bytes() != Some(b"activate") {
        return Some(Ok(()));
    }
    let stylesheet = asked.get("stylesheet").map(Value::to_php_string);
    let nonce = format!(
        "switch-theme_{}",
        String::from_utf8_lossy(&stylesheet.unwrap_or_default())
    );
    Some(referer(site, asked, &nonce))
}

/// The Ajax actions `admin-ajax.php` answers for a user logged in, by the
/// method that registers them.
const AJAX_GET: [&str; 9] = [
    "fetch-list",
    "ajax-tag-search",
    "wp-compression-test",
    "imgedit-preview",
    "oembed-cache",
    "autocomplete-user",
    "dashboard-widgets",
    "logged-in",
    "rest-nonce",
];

const AJAX_POST: [&str; 87] = [
    "oembed-cache",
    "image-editor",
    "delete-comment",
    "delete-tag",
    "delete-link",
    "delete-meta",
    "delete-post",
    "trash-post",
    "untrash-post",
    "delete-page",
    "dim-comment",
    "add-link-category",
    "add-tag",
    "get-tagcloud",
    "get-comments",
    "replyto-comment",
    "edit-comment",
    "add-menu-item",
    "add-meta",
    "add-user",
    "closed-postboxes",
    "hidden-columns",
    "update-welcome-panel",
    "menu-get-metabox",
    "wp-link-ajax",
    "menu-locations-save",
    "menu-quick-search",
    "meta-box-order",
    "get-permalink",
    "sample-permalink",
    "inline-save",
    "inline-save-tax",
    "find_posts",
    "widgets-order",
    "save-widget",
    "delete-inactive-widgets",
    "set-post-thumbnail",
    "date_format",
    "time_format",
    "wp-remove-post-lock",
    "dismiss-wp-pointer",
    "upload-attachment",
    "get-attachment",
    "query-attachments",
    "save-attachment",
    "save-attachment-compat",
    "send-link-to-editor",
    "send-attachment-to-editor",
    "save-attachment-order",
    "media-create-image-subsizes",
    "heartbeat",
    "get-revision-diffs",
    "save-user-color-scheme",
    "update-widget",
    "query-themes",
    "parse-embed",
    "set-attachment-thumbnail",
    "parse-media-shortcode",
    "destroy-sessions",
    "install-plugin",
    "activate-plugin",
    "update-plugin",
    "crop-image",
    "generate-password",
    "save-wporg-username",
    "delete-plugin",
    "search-plugins",
    "search-install-plugins",
    "update-theme",
    "delete-theme",
    "install-theme",
    "get-post-thumbnail-html",
    "get-community-events",
    "edit-theme-plugin-file",
    "wp-privacy-export-personal-data",
    "wp-privacy-erase-personal-data",
    "health-check-site-status-result",
    "health-check-dotorg-communication",
    "health-check-is-in-debug-mode",
    "health-check-background-updates",
    "health-check-loopback-requests",
    "health-check-get-sizes",
    "toggle-auto-updates",
    "send-password-reset",
    "wp-fullscreen-save-post",
    "press-this-save-post",
    "press-this-add-category",
];

/// The color schemes `register_admin_color_schemes()` registers.
const COLOR_SCHEMES: [&str; 9] = [
    "fresh",
    "light",
    "modern",
    "blue",
    "midnight",
    "sunrise",
    "ectoplasm",
    "ocean",
    "coffee",
];

/// `admin-ajax.php`: the action's checks for the logged in user, if any.
pub fn admin_ajax(site: Option<&dyn ActionSite>, asked: &Asked) -> Checked {
    // An empty action, `''` or `'0'`, is refused as one not registered.
    let action = match asked.request("action") {
        Some(action @ Value::String(_)) => action.to_php_string(),
        _ => return Err(ajax(400, "0")),
    };
    let action = String::from_utf8_lossy(&action).into_owned();
    let Some(site) = site else {
        return match action.as_str() {
            "generate-password" | "heartbeat" => Ok(()),
            _ => Err(ajax(400, "0")),
        };
    };
    // The Customizer's actions, registered when it loads.
    if ["wp_customize", "customize_changeset_uuid"]
        .iter()
        .any(|n| asked.request(n).is_some())
    {
        return Ok(());
    }
    let registered = |given: Option<&Value>, list: &[&str]| {
        given.and_then(Value::as_bytes) == Some(action.as_bytes()) && list.contains(&&*action)
    };
    let known = action == "check_plugin_dependencies"
        || registered(asked.get("action"), &AJAX_GET)
        || registered(asked.post("action"), &AJAX_POST);
    if !known {
        return Err(ajax(400, "0"));
    }
    ajax_action(site, asked, &action)
}

/// `wp_die( -1 )` unless `can`.
fn allowed(can: bool) -> Checked {
    if can { Ok(()) } else { Err(ajax(200, "-1")) }
}

/// `$_POST[ $name ]` as `(int)` casts it, 0 when not set.
fn post_int(asked: &Asked, name: &str) -> i64 {
    asked.post(name).map_or(0, Value::to_int)
}

/// The screen a user setting is saved for: `wp_die( 0 )` unless
/// `sanitize_key()` keeps it.
fn screen_page(asked: &Asked) -> Checked {
    let page = asked.post("page").map_or(Some(&b""[..]), Value::as_bytes);
    match page.is_some_and(|page| sanitize_key(page).as_bytes() == page) {
        true => Ok(()),
        false => Err(ajax(200, "0")),
    }
}

fn ajax_action(site: &dyn ActionSite, asked: &Asked, action: &str) -> Checked {
    match action {
        "delete-tag" => {
            let id = post_int(asked, "tag_ID");
            ajax_referer(site, asked, &format!("delete-tag_{id}"), None)?;
            allowed(can_for(site, "delete_term", id))
        }
        "add-tag" => {
            ajax_referer(site, asked, "add-tag", Some("_wpnonce_add-tag"))?;
            let taxonomy = asked.post("taxonomy").filter(|t| !t.is_empty());
            let taxonomy = taxonomy.map_or(b"post_tag".to_vec(), Value::to_php_string);
            // An unknown taxonomy is a fatal error in PHP.
            let cap = taxonomy_cap(&String::from_utf8_lossy(&taxonomy), "edit_terms");
            cap.map_or(Ok(()), |cap| allowed(site.current_user_can(&cap, &[])))
        }
        "inline-save-tax" => {
            ajax_referer(site, asked, "taxinlineeditnonce", Some("_inline_edit"))?;
            let taxonomy = asked
                .post("taxonomy")
                .and_then(Value::as_bytes)
                .map(sanitize_key);
            if taxonomy_cap(&taxonomy.unwrap_or_default(), "edit_terms").is_none() {
                return Err(ajax(200, "0"));
            }
            let id = post_int(asked, "tax_ID");
            allowed(id != 0 && can_for(site, "edit_term", id))
        }
        "inline-save" => {
            ajax_referer(site, asked, "inlineeditnonce", Some("_inline_edit"))?;
            let id = post_int(asked, "post_ID");
            if id == 0 {
                return Err(ajax(200, ""));
            }
            let (cap, message) = match asked.post("post_type").and_then(Value::as_bytes) {
                Some(b"page") => ("edit_page", "Sorry, you are not allowed to edit this page."),
                _ => ("edit_post", "Sorry, you are not allowed to edit this post."),
            };
            // The lock's answer is not ported.
            match can_for(site, cap, id) {
                true => Ok(()),
                false => Err(ajax(200, message)),
            }
        }
        "delete-comment" | "dim-comment" => comment_ajax(site, asked, action == "dim-comment"),
        "closed-postboxes" => {
            ajax_referer(site, asked, "closedpostboxes", Some("closedpostboxesnonce"))?;
            // A list given as an array is a fatal error in PHP.
            let lists = ["closed", "hidden"].map(|name| asked.post(name));
            match lists
                .iter()
                .any(|list| matches!(list, Some(Value::Array(_))))
            {
                true => Ok(()),
                false => screen_page(asked),
            }
        }
        "hidden-columns" => {
            ajax_referer(
                site,
                asked,
                "screen-options-nonce",
                Some("screenoptionnonce"),
            )?;
            screen_page(asked)
        }
        "meta-box-order" => {
            ajax_referer(site, asked, "meta-box-order", None)?;
            screen_page(asked)
        }
        "update-welcome-panel" => {
            ajax_referer(
                site,
                asked,
                "welcome-panel-nonce",
                Some("welcomepanelnonce"),
            )?;
            allowed(site.current_user_can("edit_theme_options", &[]))
        }
        "save-user-color-scheme" => {
            ajax_referer(site, asked, "save-color-scheme", Some("nonce"))?;
            let scheme = asked
                .post("color_scheme")
                .and_then(Value::as_bytes)
                .map(sanitize_key);
            match COLOR_SCHEMES.contains(&scheme.unwrap_or_default().as_str()) {
                true => Ok(()),
                false => Err(Refusal {
                    status: 200,
                    body: Body::Json("{\"success\":false}"),
                }),
            }
        }
        _ => Ok(()),
    }
}

/// `wp_ajax_delete_comment()` and `wp_ajax_dim_comment()`: a comment
/// missing, or the status it has already, is answered with what is not
/// ported.
fn comment_ajax(site: &dyn ActionSite, asked: &Asked, dim: bool) -> Checked {
    let id = post_int(asked, "id");
    let Some(approved) = u64::try_from(id)
        .ok()
        .and_then(|id| site.comment_approved(id))
    else {
        return Ok(());
    };
    allowed(
        can_for(site, "edit_comment", id) || dim && site.current_user_can("moderate_comments", &[]),
    )?;
    // `wp_get_comment_status()`.
    let status: Option<&[u8]> = match approved.as_slice() {
        b"1" => Some(b"approved"),
        b"0" => Some(b"unapproved"),
        b"spam" => Some(b"spam"),
        b"trash" => Some(b"trash"),
        _ => None,
    };
    if dim && status.is_some() && asked.post("new").and_then(Value::as_bytes) == status {
        return Ok(());
    }
    let nonce = if dim {
        "approve-comment"
    } else {
        "delete-comment"
    };
    ajax_referer(site, asked, &format!("{nonce}_{id}"), None)
}

#[cfg(test)]
mod tests;
