// SPDX-License-Identifier: AGPL-3.0-or-later

//! Users: `wp_insert_user()`, `wp_update_user()`, `wp_delete_user()` and
//! `WP_User`'s role methods, on a single site. The hooks of an admin request
//! (new user pointers) and the mails of `wp_update_user()` are not ported.

use hyscribe_auth::{generate_password, hash_password, wp_fast_hash};
use hyscribe_db::{Meta, UserFields, Write};
use hyscribe_kses::{esc_url_raw, wp_strip_all_tags};
use hyscribe_model::hooks::{apply_filters, do_action};
use hyscribe_phpser::{Key, Value, addslashes, is_empty_string, md5, php_trim, stripslashes};
use hyscribe_render::date::{format_unix, site_zone};
use hyscribe_render::formatting::{
    sanitize_email, sanitize_title, sanitize_user, sanitize_user_strict,
};
use hyscribe_render::kses::{wp_kses_data, wp_kses_user_description};

use crate::options::{
    Autoload, add_metadata, delete_metadata, delete_object_meta, get_metadata, get_option,
    update_metadata, update_option,
};
use crate::steps::{Steps, step};
use crate::{Error, MYSQL, name_filters, now, wp_filter_kses};

mod roles;

pub use roles::set_role;

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

/// The user meta `wp_insert_user()` writes, in its order, slashed.
fn user_meta(
    data: &UserData,
    login: &[u8],
    update: bool,
) -> (Vec<u8>, Vec<(&'static str, Vec<u8>)>) {
    let nickname = name_filters(&or(&data.nickname, login));
    let first = name_filters(&or(&data.first_name, b""));
    let last = name_filters(&or(&data.last_name, b""));
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
    (name_filters(&display), meta)
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
    let mut steps = Steps::new(write);
    let id = insert(&mut steps, data, pass).await;
    steps.finish(id)
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
async fn insert(
    steps: &mut Steps<'_, '_, Error>,
    data: &UserData,
    pass: Pass<'_>,
) -> Result<u64, Error> {
    let hash = match pass {
        Pass::Plain(pass) => hash_password(pass).map(Some),
        Pass::Hashed(hash) => hash.map(Some),
        Pass::Kept => Ok(None),
    };
    let found = match data.id {
        0 => None,
        id => step!(steps, user(id)),
    };
    let hash = steps.keep(hash.map_err(Error::Random));
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
    if old.is_none() && step!(steps, user_id_by_login(&login)).is_some() {
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
    if step!(steps, nicename_owner(&nicename, &login)).is_some() {
        let base = String::from_utf8_lossy(&nicename).into_owned();
        let mut suffix = 2;
        nicename = loop {
            let length = 49 - suffix.to_string().len();
            let alt = format!("{}-{suffix}", base.chars().take(length).collect::<String>());
            if step!(steps, nicename_owner(alt.as_bytes(), &login)).is_none() {
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
        && step!(steps, user_id_by_email(email_id)).is_some()
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
        _ => format_unix(MYSQL, now(), site_zone(b"", b"0")).into_bytes(),
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
            step!(steps, update_user(data.id, &fields));
            data.id
        }
        None => step!(steps, insert_user(&login, &fields)),
    };
    let meta = meta
        .into_iter()
        .map(|(key, value)| (key.to_string(), Value::String(value)));
    for (key, value) in merge(meta.collect(), &data.meta_input) {
        if old.is_some() {
            let none = &Value::Null;
            steps
                .run(async |w| update_metadata(w, Meta::User, id, &key, &value, none).await)
                .await;
        } else {
            steps
                .run(async |w| add_metadata(w, Meta::User, id, &key, &value, false).await)
                .await;
        }
    }
    let role = match &data.role {
        Some(role) => Some(role.clone()),
        None if old.is_none() => {
            let role = steps.run(async |w| get_option(w, "default_role").await);
            Some(role.await.to_php_string())
        }
        None => None,
    };
    if let Some(role) = role {
        steps.run(async |w| set_role(w, id, &role).await).await;
    }
    if old.is_none() {
        update_user_count(steps).await;
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
async fn update_user_count(steps: &mut Steps<'_, '_, Error>) {
    // A missing count reads as 0, where PHP's default is -1: both are small.
    let known = steps.run(async |w| get_option(w, "user_count").await);
    if known.await.to_int() <= 10_000 {
        let count = step!(steps, user_count());
        let count = Value::String(count.to_string().into_bytes());
        let off = Some(Autoload::Off);
        steps
            .run(async |w| update_option(w, "user_count", &count, off).await)
            .await;
    }
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
    let mut steps = Steps::new(write);
    let id = update(&mut steps, data).await;
    steps.finish(id)
}

async fn update(steps: &mut Steps<'_, '_, Error>, data: &UserData) -> Result<u64, Error> {
    let Some((login, user)) = step!(steps, user(data.id)) else {
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
        let values = steps.run(async |w| get_metadata(w, Meta::User, data.id, key).await);
        let values = values.await;
        let value = values.first().map(Value::to_php_string).unwrap_or_default();
        fill(field, &value);
    }
    insert(steps, &merged, Pass::Hashed(pass)).await
}

/// A password reset key kept for a user, for the mail that carries it.
#[derive(Debug)]
pub struct ResetKey {
    /// The user's ID.
    pub id: u64,
    /// `user_login`.
    pub login: Vec<u8>,
    /// `user_email`.
    pub email: Vec<u8>,
    /// The key, whose fast hash is stored.
    pub key: String,
}

/// A `WP_Error` of `retrieve_password()`: its code and message.
#[derive(Debug)]
pub struct Refusal {
    /// The error code.
    pub code: &'static str,
    /// The message, as markup.
    pub message: &'static str,
}

/// The message of a login or address no user has.
const NO_ACCOUNT: &str =
    "<strong>Error:</strong> There is no account with that username or email address.";

/// `get_password_reset_key()` of the user `id`: a new key's fast hash
/// stored after the time as its `user_activation_key`, with the actions
/// `retrieve_password` and `retrieve_password_key`.
pub async fn get_password_reset_key(write: &mut Write<'_>, id: u64) -> Result<ResetKey, Error> {
    let mut steps = Steps::new(write);
    let (login, user) = step!(steps, user(id)).unwrap_or_default();
    do_action("retreive_password", &[&login]);
    do_action("retrieve_password", &[&login]);
    let key = steps.keep(generate_password(20, false).map_err(Error::Random));
    do_action("retrieve_password_key", &[&login, &key]);
    let hashed = format!("{}:{}", now(), wp_fast_hash(key.as_bytes()));
    let data = UserData {
        id,
        user_activation_key: Some(hashed.into_bytes()),
        ..UserData::default()
    };
    steps.run(async |w| wp_update_user(w, &data).await).await;
    let email = user.email;
    steps.finish(Ok(ResetKey {
        id,
        login,
        email,
        key,
    }))
}

/// `retrieve_password( $user_login )` up to its mail, the login slashed as
/// `$_POST` holds it: the user's new reset key, by their email when the
/// login has an `@` past its first character, else the error PHP gives.
pub async fn retrieve_password(
    write: &mut Write<'_>,
    login: &[u8],
) -> Result<Result<ResetKey, Refusal>, Error> {
    let login = stripslashes(login);
    let login = php_trim(&login);
    let (mut user, mut errors) = (None, None);
    let by_login =
        async |write: &mut Write<'_>| write.user_id_by_login(&sanitize_user(login)).await;
    if is_empty_string(login) {
        errors = Some(Refusal {
            code: "empty_username",
            message: "<strong>Error:</strong> Please enter a username or email address.",
        });
    } else if login
        .iter()
        .position(|&b| b == b'@')
        .is_some_and(|at| at > 0)
    {
        user = write.user_id_by_email(login).await?;
        if user.is_none() {
            user = by_login(write).await?;
        }
        if user.is_none() {
            errors = Some(Refusal {
                code: "invalid_email",
                message: NO_ACCOUNT,
            });
        }
    } else {
        user = by_login(write).await?;
    }
    let user = apply_filters("lostpassword_user_data", user, &[&errors]);
    do_action("lostpassword_post", &[&errors, &user]);
    let errors = apply_filters("lostpassword_errors", errors, &[&user]);
    match (errors, user) {
        (Some(refusal), _) => Ok(Err(refusal)),
        (None, None) => Ok(Err(Refusal {
            code: "invalidcombo",
            message: NO_ACCOUNT,
        })),
        (None, Some(id)) => get_password_reset_key(write, id).await.map(Ok),
    }
}

/// `reset_password()` of the user `id` with a slashed password: the hash set,
/// the key cleared and the nag emptied; the login and email, for the notice.
pub async fn reset_password(
    write: &mut Write<'_>,
    id: u64,
    password: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), Error> {
    // The hooks get the password as PHP gives it, slashed.
    let new_pass = password.to_vec();
    do_action("password_reset", &[&id, &new_pass]);
    let mut steps = Steps::new(write);
    let (login, mut user) = step!(steps, user(id)).unwrap_or_default();
    user.pass = steps.keep(hash_password(password).map_err(Error::Random));
    user.activation_key.clear();
    step!(steps, update_user(id, &user));
    do_action("wp_set_password", &[&new_pass, &id]);
    let (nag, none) = (Value::Bool(false), Value::Null);
    let meta = async |w: &mut Write<'_>| {
        update_metadata(w, Meta::User, id, "default_password_nag", &nag, &none).await
    };
    steps.run(meta).await;
    do_action("after_password_reset", &[&id, &new_pass]);
    steps.finish(Ok((login, user.email)))
}

/// `send_confirmation_on_profile_email()`'s rows for the user `id` asking
/// for `email`, valid and not its own, slashed as `$_POST` holds it: the
/// change waits in `_new_email` under a random hash for its confirmation,
/// or none waits when another user has the address. The hash waiting,
/// which the confirmation mail carries.
pub async fn request_email_change(
    write: &mut Write<'_>,
    id: u64,
    email: &[u8],
) -> Result<Option<Vec<u8>>, Error> {
    let mut steps = Steps::new(write);
    let key = "_new_email";
    let mut waiting = None;
    if step!(steps, user_id_by_email(php_trim(email))).is_some() {
        let none = &Value::Null;
        let deleted =
            async |w: &mut Write<'_>| delete_metadata(w, Meta::User, id, key, none, false).await;
        steps.run(deleted).await;
    } else {
        let random = steps.keep(generate_password(20, false).map_err(Error::Random));
        let seed = [email, now().to_string().as_bytes(), random.as_bytes()].concat();
        let hash = md5(&seed).into_bytes();
        let change = Value::Array(vec![
            (Key::String(b"hash".to_vec()), Value::String(hash.clone())),
            (
                Key::String(b"newemail".to_vec()),
                Value::String(email.to_vec()),
            ),
        ]);
        waiting = Some(hash);
        let none = &Value::Null;
        let kept =
            async |w: &mut Write<'_>| update_metadata(w, Meta::User, id, key, &change, none).await;
        steps.run(kept).await;
    }
    steps.finish(Ok(waiting))
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
    let mut steps = Steps::new(write);
    let deleted = delete(&mut steps, id, reassign, request).await;
    steps.finish(deleted)
}

async fn delete(
    steps: &mut Steps<'_, '_, Error>,
    id: u64,
    reassign: Option<u64>,
    request: &crate::posts::Context,
) -> Result<bool, Error> {
    if step!(steps, user(id)).is_none() {
        return Ok(false);
    }
    match reassign {
        Some(heir) => {
            step!(steps, reassign_posts(id, heir));
            step!(steps, reassign_links(id, heir));
        }
        None => {
            for post in step!(steps, author_post_ids_of_types(id, &DELETED_WITH_USER)) {
                let deleted = async |w: &mut Write<'_>| {
                    crate::posts::wp_delete_post(w, request, post, false).await
                };
                steps.run(deleted).await;
            }
            // `wp_delete_link()` of each link, once ported.
            if !step!(steps, owner_link_ids(id)).is_empty() {
                return Err(Error::Unported("wp_delete_link"));
            }
        }
    }
    steps
        .run(async |w| delete_object_meta(w, Meta::User, id).await)
        .await;
    step!(steps, delete_user(id));
    update_user_count(steps).await;
    Ok(true)
}

#[cfg(test)]
mod tests;
