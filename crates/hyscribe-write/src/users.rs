// SPDX-License-Identifier: AGPL-3.0-or-later

//! Users: `wp_insert_user()`, `wp_update_user()`, `wp_delete_user()` and
//! `WP_User`'s role methods, on a single site. The hooks of an admin request
//! (new user pointers) and the mails of `wp_update_user()` are not ported.

use hyscribe_auth::hash_password;
use hyscribe_db::{Meta, UserFields, Write};
use hyscribe_kses::{esc_url_raw, wp_specialchars, wp_strip_all_tags};
use hyscribe_phpser::{Value, addslashes, is_empty_string, php_trim, stripslashes};
use hyscribe_render::date::{format_unix, site_zone};
use hyscribe_render::formatting::{
    sanitize_email, sanitize_text_field, sanitize_title, sanitize_user_strict,
};
use hyscribe_render::kses::{wp_kses_data, wp_kses_user_description};

use crate::options::{
    Autoload, add_metadata, get_metadata, get_option, update_metadata, update_option,
};
use crate::{Error, now};

mod roles;

pub use roles::{add_role, remove_role, set_role};

/// The post types whose posts `wp_delete_user()` deletes with their author:
/// those with `delete_with_user`, or with it unset and supporting authors.
const DELETED_WITH_USER: [&str; 6] = [
    "post",
    "page",
    "attachment",
    "revision",
    "wp_template",
    "wp_template_part",
];

/// `wp_insert_user()`'s `$userdata`, each field `None` where the array lacks
/// it. Values are as PHP receives them; the meta functions unslash them.
/// A new user's `user_pass` is the plain password, hashed and never logged;
/// `wp_insert_user()` stores an update's as given, as PHP does.
#[derive(Clone, Default)]
pub struct UserData {
    /// `ID`: the user to update, 0 for a new one.
    pub id: u64,
    /// `user_pass`.
    pub user_pass: Option<Vec<u8>>,
    /// `user_login`.
    pub user_login: Option<Vec<u8>>,
    /// `user_nicename`.
    pub user_nicename: Option<Vec<u8>>,
    /// `user_url`.
    pub user_url: Option<Vec<u8>>,
    /// `user_email`.
    pub user_email: Option<Vec<u8>>,
    /// `display_name`.
    pub display_name: Option<Vec<u8>>,
    /// `nickname`.
    pub nickname: Option<Vec<u8>>,
    /// `first_name`.
    pub first_name: Option<Vec<u8>>,
    /// `last_name`.
    pub last_name: Option<Vec<u8>>,
    /// `description`.
    pub description: Option<Vec<u8>>,
    /// `rich_editing`.
    pub rich_editing: Option<Vec<u8>>,
    /// `syntax_highlighting`.
    pub syntax_highlighting: Option<Vec<u8>>,
    /// `infinite_scrolling`.
    pub infinite_scrolling: Option<Vec<u8>>,
    /// `comment_shortcuts`.
    pub comment_shortcuts: Option<Vec<u8>>,
    /// `admin_color`.
    pub admin_color: Option<Vec<u8>>,
    /// `use_ssl`.
    pub use_ssl: Option<Vec<u8>>,
    /// `user_registered`, `YYYY-MM-DD hh:mm:ss`.
    pub user_registered: Option<Vec<u8>>,
    /// `user_activation_key`.
    pub user_activation_key: Option<Vec<u8>>,
    /// `show_admin_bar_front`.
    pub show_admin_bar_front: Option<Vec<u8>>,
    /// `locale`.
    pub locale: Option<Vec<u8>>,
    /// `role`.
    pub role: Option<Vec<u8>>,
    /// `meta_input`: more user meta, by key.
    pub meta_input: Vec<(String, Value)>,
}

/// PHP's `empty()` of a field.
fn empty(field: &Option<Vec<u8>>) -> bool {
    field.as_deref().is_none_or(is_empty_string)
}

/// The field, or `default` where it is empty.
fn or(field: &Option<Vec<u8>>, default: &[u8]) -> Vec<u8> {
    match field {
        Some(value) if !is_empty_string(value) => value.clone(),
        _ => default.to_vec(),
    }
}

/// `mb_strlen()`.
fn mb_strlen(text: &[u8]) -> usize {
    String::from_utf8_lossy(text).chars().count()
}

/// `wp_filter_kses()`: kses over the unslashed text, slashed again.
fn wp_filter_kses(text: &[u8], kses: fn(&[u8]) -> Vec<u8>) -> Vec<u8> {
    addslashes(&kses(&stripslashes(text)))
}

/// The default `pre_user_{name}` filters of names: `sanitize_text_field()`,
/// `wp_filter_kses()` and `_wp_specialchars()`.
fn pre_user_name(text: &[u8]) -> Vec<u8> {
    wp_specialchars(&wp_filter_kses(&sanitize_text_field(text), wp_kses_data))
}

/// The user meta `wp_insert_user()` writes, in its order, slashed.
fn user_meta(
    data: &UserData,
    login: &[u8],
    update: bool,
) -> (Vec<u8>, Vec<(&'static str, Vec<u8>)>) {
    let nickname = pre_user_name(&or(&data.nickname, login));
    let first = pre_user_name(&or(&data.first_name, b""));
    let last = pre_user_name(&or(&data.last_name, b""));
    let display = match &data.display_name {
        Some(name) if !is_empty_string(name) => name.clone(),
        _ if update => login.to_vec(),
        _ => match (is_empty_string(&first), is_empty_string(&last)) {
            (false, false) => [&first[..], b" ", &last].concat(),
            (false, true) => first.clone(),
            (true, false) => last.clone(),
            (true, true) => login.to_vec(),
        },
    };
    let comment_shortcuts = match data.comment_shortcuts.as_deref() {
        Some(value) if !is_empty_string(value) && value != b"false" => b"true".to_vec(),
        _ => b"false".to_vec(),
    };
    let mut admin_color = or(&data.admin_color, b"modern");
    admin_color.retain(|b| b.is_ascii_alphanumeric() || b" _.-@".contains(b));
    let meta = vec![
        ("nickname", nickname),
        ("first_name", first),
        ("last_name", last),
        (
            "description",
            wp_filter_kses(&or(&data.description, b""), wp_kses_user_description),
        ),
        ("rich_editing", or(&data.rich_editing, b"true")),
        (
            "syntax_highlighting",
            or(&data.syntax_highlighting, b"true"),
        ),
        ("infinite_scrolling", or(&data.infinite_scrolling, b"true")),
        ("comment_shortcuts", comment_shortcuts),
        ("admin_color", admin_color),
        (
            "use_ssl",
            if empty(&data.use_ssl) { b"0" } else { b"1" }.to_vec(),
        ),
        (
            "show_admin_bar_front",
            or(&data.show_admin_bar_front, b"true"),
        ),
        ("locale", data.locale.clone().unwrap_or_default()),
    ];
    (pre_user_name(&display), meta)
}

/// The text of a column `wp_unslash()` gives.
fn unslashed_text(text: &[u8]) -> String {
    String::from_utf8_lossy(&stripslashes(text)).into_owned()
}

/// `wp_insert_user( $userdata )`: a new user, its password hashed, or an
/// update of the user `id`, whose `user_pass` PHP takes for a hash and
/// stores as given (`wp_update_user()` hashes a new password). The ID.
pub async fn wp_insert_user(write: &mut Write<'_>, data: &UserData) -> Result<u64, Error> {
    let pass = data.user_pass.as_deref().unwrap_or_default();
    let pass = match data.id {
        0 => Pass::Plain(pass),
        _ if is_empty_string(pass) => Pass::Kept,
        _ => Pass::Hashed(Ok(String::from_utf8_lossy(pass).into_owned())),
    };
    insert(write, data, pass).await
}

/// The password `wp_insert_user()` stores.
enum Pass<'a> {
    /// A password to hash.
    Plain(&'a [u8]),
    /// A hash, or why none was made.
    Hashed(std::io::Result<String>),
    /// The user's own hash.
    Kept,
}

/// `wp_insert_user()` storing `pass`.
async fn insert(write: &mut Write<'_>, data: &UserData, pass: Pass<'_>) -> Result<u64, Error> {
    let hash = match pass {
        Pass::Plain(pass) => hash_password(pass).map(Some),
        Pass::Hashed(hash) => hash.map(Some),
        Pass::Kept => Ok(None),
    };
    let found = match data.id {
        0 => Ok(None),
        id => write.user(id).await,
    };
    // One `?` for either error.
    let found = found.map_err(Error::Db);
    let (found, hash) =
        found.and_then(|found| hash.map(|hash| (found, hash)).map_err(Error::Random))?;
    let old = match data.id {
        0 => None,
        _ => Some(found.ok_or(Error::Refused("invalid_user_id"))?.1),
    };
    let pass = hash
        .or_else(|| old.as_ref().map(|old| old.pass.clone()))
        .unwrap_or_default();
    let login = sanitize_user_strict(data.user_login.as_deref().unwrap_or_default());
    if is_empty_string(&login) {
        return Err(Error::Refused("empty_user_login"));
    }
    if login.len() > 60 {
        return Err(Error::Refused("user_login_too_long"));
    }
    if old.is_none() && write.user_id_by_login(&login).await?.is_some() {
        return Err(Error::Refused("existing_user_login"));
    }
    let nicename = match &data.user_nicename {
        Some(name) if !is_empty_string(name) => sanitize_user_strict(name),
        _ => login.iter().copied().take(50).collect(),
    };
    let mut nicename = sanitize_title(&nicename);
    if is_empty_string(&nicename) {
        return Err(Error::Refused("empty_user_nicename"));
    }
    if mb_strlen(&nicename) > 50 {
        return Err(Error::Refused("user_nicename_too_long"));
    }
    if write.nicename_owner(&nicename, &login).await?.is_some() {
        let base = String::from_utf8_lossy(&nicename).into_owned();
        let mut suffix = 2;
        nicename = loop {
            let length = 49 - suffix.to_string().len();
            let alt = format!("{}-{suffix}", base.chars().take(length).collect::<String>());
            if write
                .nicename_owner(alt.as_bytes(), &login)
                .await?
                .is_none()
            {
                break alt.into_bytes();
            }
            suffix += 1;
        };
    }
    let email = wp_filter_kses(
        &sanitize_email(php_trim(&or(&data.user_email, b""))),
        wp_kses_data,
    );
    let old_email = old.as_ref().map(|old| addslashes(&old.email));
    let changed_email = old_email
        .as_ref()
        .is_none_or(|old| !old.eq_ignore_ascii_case(&email));
    let email_id = php_trim(&email);
    if changed_email
        && !is_empty_string(email_id)
        && write.user_id_by_email(email_id).await?.is_some()
    {
        return Err(Error::Refused("existing_user_email"));
    }
    let url = or(&data.user_url, b"");
    let url = wp_filter_kses(&esc_url_raw(&wp_strip_all_tags(&url, false)), wp_kses_data);
    if mb_strlen(&url) > 100 {
        return Err(Error::Refused("user_url_too_long"));
    }
    let registered = match &data.user_registered {
        Some(date) if !is_empty_string(date) => date.clone(),
        _ => format_unix("Y-m-d H:i:s", now(), site_zone(b"", b"0")).into_bytes(),
    };
    let (display_name, meta) = user_meta(data, &login, old.is_some());
    let mut fields = UserFields {
        pass,
        nicename: stripslashes(&nicename),
        email: stripslashes(&email),
        url: stripslashes(&url),
        registered: unslashed_text(&registered),
        activation_key: unslashed_text(&or(&data.user_activation_key, b"")),
        display_name: stripslashes(&display_name),
    };
    let id = match &old {
        Some(old) => {
            if old_email.as_deref() != Some(&email[..]) || fields.pass != old.pass {
                fields.activation_key.clear();
            }
            write.update_user(data.id, &fields).await?;
            data.id
        }
        None => write.insert_user(&login, &fields).await?,
    };
    let meta = meta
        .into_iter()
        .map(|(key, value)| (key.to_string(), Value::String(value)));
    for (key, value) in merge(meta.collect(), &data.meta_input) {
        if old.is_some() {
            update_metadata(write, Meta::User, id, &key, &value, &Value::Null).await?;
        } else {
            add_metadata(write, Meta::User, id, &key, &value, false).await?;
        }
    }
    match &data.role {
        Some(role) => set_role(write, id, role).await?,
        None if old.is_none() => {
            let role = get_option(write, "default_role").await?.to_php_string();
            set_role(write, id, &role).await?;
        }
        None => {}
    }
    if old.is_none() {
        update_user_count(write).await?;
    }
    Ok(id)
}

/// `array_merge()` of string keys: a key `more` repeats takes its value in
/// place, new keys follow.
fn merge(mut meta: Vec<(String, Value)>, more: &[(String, Value)]) -> Vec<(String, Value)> {
    for (key, value) in more {
        match meta.iter_mut().find(|(name, _)| name == key) {
            Some(entry) => entry.1 = value.clone(),
            None => meta.push((key.clone(), value.clone())),
        }
    }
    meta
}

/// `wp_maybe_update_user_counts()` after a user is added or deleted: the
/// `user_count` option, unless it is over 10000.
async fn update_user_count(write: &mut Write<'_>) -> Result<(), Error> {
    // A missing count reads as 0, where PHP's default is -1: both are small.
    if get_option(write, "user_count").await?.to_int() <= 10_000 {
        let count = write.user_count().await?;
        let count = Value::String(count.to_string().into_bytes());
        update_option(write, "user_count", &count, Some(Autoload::Off)).await?;
    }
    Ok(())
}

/// The meta `wp_update_user()` reads back into `$userdata`.
const ADDITIONAL_KEYS: [&str; 12] = [
    "first_name",
    "last_name",
    "nickname",
    "description",
    "rich_editing",
    "syntax_highlighting",
    "infinite_scrolling",
    "comment_shortcuts",
    "admin_color",
    "use_ssl",
    "show_admin_bar_front",
    "locale",
];

/// `wp_update_user( $userdata )`: `data`'s fields over the user's own, the
/// new password hashed. The user's ID.
pub async fn wp_update_user(write: &mut Write<'_>, data: &UserData) -> Result<u64, Error> {
    let Some((login, user)) = write.user(data.id).await? else {
        return Err(Error::Refused("invalid_user_id"));
    };
    let pass = match data.user_pass.as_deref() {
        Some(pass) if !is_empty_string(pass) && pass != user.pass.as_bytes() => hash_password(pass),
        _ => Ok(user.pass.clone()),
    };
    let mut merged = data.clone();
    // `add_magic_quotes()` on the user's own values.
    let fill = |field: &mut Option<Vec<u8>>, value: &[u8]| {
        field.get_or_insert_with(|| addslashes(value));
    };
    fill(&mut merged.user_login, &login);
    fill(&mut merged.user_nicename, &user.nicename);
    fill(&mut merged.user_email, &user.email);
    fill(&mut merged.user_url, &user.url);
    fill(&mut merged.user_registered, user.registered.as_bytes());
    fill(
        &mut merged.user_activation_key,
        user.activation_key.as_bytes(),
    );
    fill(&mut merged.display_name, &user.display_name);
    let fields = [
        &mut merged.first_name,
        &mut merged.last_name,
        &mut merged.nickname,
        &mut merged.description,
        &mut merged.rich_editing,
        &mut merged.syntax_highlighting,
        &mut merged.infinite_scrolling,
        &mut merged.comment_shortcuts,
        &mut merged.admin_color,
        &mut merged.use_ssl,
        &mut merged.show_admin_bar_front,
        &mut merged.locale,
    ];
    for (key, field) in ADDITIONAL_KEYS.into_iter().zip(fields) {
        let values = get_metadata(write, Meta::User, data.id, key).await?;
        let value = values.first().map(Value::to_php_string).unwrap_or_default();
        fill(field, &value);
    }
    insert(write, &merged, Pass::Hashed(pass)).await
}

/// `wp_delete_user( $id, $reassign )`: the user and its meta, its posts and
/// links passed to `reassign`, or deleted with it as `request` deletes them
/// (posts and pages to the trash). False for no such user.
pub async fn wp_delete_user(
    write: &mut Write<'_>,
    id: u64,
    reassign: Option<u64>,
    request: &crate::posts::Context,
) -> Result<bool, Error> {
    if write.user(id).await?.is_none() {
        return Ok(false);
    }
    match reassign {
        Some(heir) => {
            write.reassign_posts(id, heir).await?;
            write.reassign_links(id, heir).await?;
        }
        None => {
            for post in write
                .author_post_ids_of_types(id, &DELETED_WITH_USER)
                .await?
            {
                crate::posts::wp_delete_post(write, request, post, false).await?;
            }
            // `wp_delete_link()` of each link, once ported.
            if !write.owner_link_ids(id).await?.is_empty() {
                return Err(Error::Unported("wp_delete_link"));
            }
        }
    }
    let meta = write.object_meta_ids(Meta::User, id).await?;
    write.delete_meta(Meta::User, &meta).await?;
    write.delete_user(id).await?;
    update_user_count(write).await?;
    Ok(true)
}

#[cfg(test)]
mod tests;
